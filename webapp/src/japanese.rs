use std::{cell::RefCell, rc::Rc, sync::OnceLock};

use stream_mecab::{
    FIRST_USER_TAG, Model, TAG_UNKNOWN_HAN, TAG_UNKNOWN_HIRAGANA, TAG_UNKNOWN_KATAKANA,
    TAG_UNKNOWN_LATIN, TAG_UNKNOWN_NUMBER, TAG_UNKNOWN_OTHER, TAG_UNKNOWN_PUNCT, TAG_UNKNOWN_SPACE,
};

use super::TokenKind;

const TAG_NOUN: u16 = FIRST_USER_TAG;
const TAG_PARTICLE: u16 = FIRST_USER_TAG + 1;
const TAG_AUXILIARY: u16 = FIRST_USER_TAG + 2;
const TAG_VERB: u16 = FIRST_USER_TAG + 3;
const TAG_ADJECTIVE: u16 = FIRST_USER_TAG + 4;
const TAG_CONNECTIVE: u16 = FIRST_USER_TAG + 5;

const LINE_CACHE_SLOTS: usize = 128;
const MAX_CACHED_LINE_BYTES: usize = 4 * 1024;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct HighlightSpan {
    start: usize,
    end: usize,
    kind: TokenKind,
}

struct CachedLine {
    hash: u64,
    source: Box<str>,
    spans: Rc<[HighlightSpan]>,
}

thread_local! {
    static LINE_CACHE: RefCell<Vec<Option<CachedLine>>> = RefCell::new(
        std::iter::repeat_with(|| None).take(LINE_CACHE_SLOTS).collect()
    );
}

// This vocabulary is deliberately small and project-authored. It is not copied
// from MeCab/IPADIC/UniDic/JUMAN (and stream-mecab does not consume those
// formats). Unknown Japanese is still classified by script, so highlighting
// remains useful without a large third-party dictionary.
const NOUNS: &[&str] = &[
    "日本語",
    "言語",
    "コード",
    "ブロック",
    "シンタックス",
    "ハイライト",
    "構文",
    "解析",
    "形態素",
    "単語",
    "文字",
    "文字列",
    "関数",
    "変数",
    "型",
    "数値",
    "コメント",
    "プログラム",
    "処理",
    "結果",
    "入力",
    "出力",
    "データ",
    "モデル",
    "辞書",
    "速度",
    "性能",
    "実装",
    "表示",
    "追加",
    "削除",
    "変更",
    "生成",
    "変換",
    "検索",
    "テスト",
    "エラー",
    "ファイル",
    "行",
    "列",
    "名前",
    "値",
    "状態",
    "要素",
    "配列",
    "時間",
    "方法",
    "問題",
    "機能",
    "情報",
    "文章",
    "文",
    "今日",
    "明日",
    "昨日",
    "東京",
    "大学",
    "学生",
    "私",
    "人",
    "もの",
    "こと",
];

const PARTICLES: &[&str] = &[
    "は",
    "が",
    "を",
    "に",
    "へ",
    "と",
    "で",
    "の",
    "も",
    "や",
    "か",
    "から",
    "まで",
    "より",
    "だけ",
    "しか",
    "など",
    "ほど",
    "くらい",
    "ぐらい",
    "こそ",
    "さえ",
    "ばかり",
    "って",
    "ので",
    "のに",
    "なら",
    "ても",
    "でも",
    "て",
    "ね",
    "よ",
];

const AUXILIARIES: &[&str] = &[
    "です",
    "でした",
    "ます",
    "ました",
    "ません",
    "だ",
    "だった",
    "である",
    "ない",
    "たい",
    "らしい",
    "そう",
    "よう",
    "れる",
    "られる",
    "せる",
    "させる",
];

const VERBS: &[&str] = &[
    "する",
    "して",
    "した",
    "いる",
    "ある",
    "なる",
    "できる",
    "でき",
    "思う",
    "言う",
    "見る",
    "使う",
    "作る",
    "読む",
    "書く",
    "行く",
    "来る",
    "分かる",
    "考える",
    "入れる",
    "出す",
    "返す",
    "呼ぶ",
    "動く",
    "動かす",
    "始める",
    "終わる",
    "選ぶ",
    "含む",
    "持つ",
];

const ADJECTIVES: &[&str] = &[
    "良い",
    "悪い",
    "高い",
    "低い",
    "速い",
    "遅い",
    "大きい",
    "小さい",
    "新しい",
    "古い",
    "難しい",
    "簡単",
    "可能",
    "必要",
    "重要",
    "安全",
    "同じ",
    "異なる",
    "多い",
    "少ない",
    "長い",
    "短い",
];

const CONNECTIVES: &[&str] = &[
    "そして",
    "しかし",
    "また",
    "さらに",
    "つまり",
    "ただし",
    "もし",
    "まず",
    "次に",
    "例えば",
    "特に",
    "すでに",
    "まだ",
    "とても",
    "かなり",
    "ほぼ",
];

pub(super) fn is_fence(language: Option<&str>) -> bool {
    let Some(language) = language.map(str::trim) else {
        return false;
    };
    language == "日本語"
        || language.eq_ignore_ascii_case("japanese")
        || language.eq_ignore_ascii_case("nihongo")
        || language.eq_ignore_ascii_case("ja")
        || language.eq_ignore_ascii_case("jp")
}

fn model() -> &'static Model {
    static MODEL: OnceLock<Model> = OnceLock::new();
    MODEL.get_or_init(|| {
        let mut model = Model::new();
        model.set_max_unknown_chars(16);
        add_words(&mut model, NOUNS, TAG_NOUN, 120);
        add_words(&mut model, PARTICLES, TAG_PARTICLE, 60);
        add_words(&mut model, AUXILIARIES, TAG_AUXILIARY, 80);
        add_words(&mut model, VERBS, TAG_VERB, 100);
        add_words(&mut model, ADJECTIVES, TAG_ADJECTIVE, 110);
        add_words(&mut model, CONNECTIVES, TAG_CONNECTIVE, 90);

        // Small grammar biases resolve overlaps such as noun + particle without
        // trying to be a full linguistic model. Missing transitions cost zero.
        for previous in [TAG_NOUN, TAG_VERB, TAG_ADJECTIVE] {
            model.set_transition(previous, TAG_PARTICLE, -80);
        }
        for next in [TAG_NOUN, TAG_VERB, TAG_ADJECTIVE] {
            model.set_transition(TAG_PARTICLE, next, -50);
        }
        model.set_transition(TAG_VERB, TAG_AUXILIARY, -60);
        model.set_transition(TAG_ADJECTIVE, TAG_AUXILIARY, -40);
        model
    })
}

fn add_words(model: &mut Model, words: &[&str], tag: u16, cost: i32) {
    for &surface in words {
        model
            .add_entry(surface, surface, "", tag, cost)
            .expect("static Japanese highlight lexicon must be valid");
    }
}

pub(super) fn highlight<'a>(source: &'a str, mut emit: impl FnMut(&'a str, TokenKind) -> bool) {
    // Analyze line by line. Japanese morphology does not need to cross a hard
    // line break, and this preserves the renderer's early-stop virtualization:
    // lower off-screen lines are never tokenized.
    for line in source.split_inclusive('\n') {
        if !highlight_line(line, &mut emit) {
            return;
        }
    }
}

fn highlight_line<'a>(line: &'a str, emit: &mut impl FnMut(&'a str, TokenKind) -> bool) -> bool {
    let hash = line_hash(line);
    if line.len() <= MAX_CACHED_LINE_BYTES {
        let slot = hash as usize & (LINE_CACHE_SLOTS - 1);
        let cached = LINE_CACHE.with(|cache| {
            let cache = cache.borrow();
            let entry = cache.get(slot)?.as_ref()?;
            (entry.hash == hash && entry.source.as_ref() == line).then(|| entry.spans.clone())
        });
        if let Some(spans) = cached {
            return emit_spans(line, &spans, emit);
        }
    }

    let spans: Rc<[HighlightSpan]> = tokenize_spans(line).into();
    let keep_going = emit_spans(line, &spans, emit);
    if line.len() <= MAX_CACHED_LINE_BYTES {
        let slot = hash as usize & (LINE_CACHE_SLOTS - 1);
        LINE_CACHE.with(|cache| {
            cache.borrow_mut()[slot] = Some(CachedLine {
                hash,
                source: line.into(),
                spans,
            });
        });
    }
    keep_going
}

fn tokenize_spans(line: &str) -> Vec<HighlightSpan> {
    let tokens = model().tokenize(line);
    let mut spans: Vec<HighlightSpan> = Vec::with_capacity(tokens.len());
    for token in tokens {
        let kind = token_kind(token.tag);
        if let Some(last) = spans.last_mut()
            && last.kind == kind
            && kind != TokenKind::Plain
            && last.end as usize == token.start
        {
            last.end = token.end;
            continue;
        }
        spans.push(HighlightSpan {
            start: token.start,
            end: token.end,
            kind,
        });
    }
    spans
}

fn emit_spans<'a>(
    line: &'a str,
    spans: &[HighlightSpan],
    emit: &mut impl FnMut(&'a str, TokenKind) -> bool,
) -> bool {
    for span in spans {
        if !emit(&line[span.start..span.end], span.kind) {
            return false;
        }
    }
    true
}

#[inline]
fn line_hash(line: &str) -> u64 {
    // This hash only selects a direct-mapped cache slot; correctness always
    // comes from the complete source comparison on a candidate hit. Sample a
    // few words instead of rescanning the whole line every rendered frame.
    let bytes = line.as_bytes();
    let len = bytes.len();
    let mut hash = (len as u64).wrapping_mul(0x9e3779b185ebca87);
    for start in [0, len / 3, len.saturating_mul(2) / 3, len.saturating_sub(8)] {
        hash ^= sampled_word(bytes, start).wrapping_mul(0xc2b2ae3d27d4eb4f);
        hash = hash.rotate_left(23).wrapping_mul(0x165667b19e3779f9);
    }
    hash ^= hash >> 29;
    hash
}

#[inline]
fn sampled_word(bytes: &[u8], start: usize) -> u64 {
    let end = (start + 8).min(bytes.len());
    let mut word = 0u64;
    for (shift, &byte) in bytes[start.min(bytes.len())..end].iter().enumerate() {
        word |= u64::from(byte) << (shift * 8);
    }
    word
}

fn token_kind(tag: u16) -> TokenKind {
    match tag {
        TAG_NOUN | TAG_UNKNOWN_HAN | TAG_UNKNOWN_KATAKANA => TokenKind::Type,
        TAG_PARTICLE | TAG_CONNECTIVE => TokenKind::Keyword,
        TAG_AUXILIARY => TokenKind::Macro,
        TAG_VERB => TokenKind::Function,
        TAG_ADJECTIVE => TokenKind::String,
        TAG_UNKNOWN_NUMBER => TokenKind::Number,
        TAG_UNKNOWN_PUNCT => TokenKind::Operator,
        TAG_UNKNOWN_HIRAGANA | TAG_UNKNOWN_LATIN | TAG_UNKNOWN_SPACE | TAG_UNKNOWN_OTHER | _ => {
            TokenKind::Plain
        }
    }
}
