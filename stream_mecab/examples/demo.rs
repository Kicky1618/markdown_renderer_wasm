use stream_mecab::{FIRST_USER_TAG, Model};

fn main() {
    let mut model = Model::new();
    let noun = FIRST_USER_TAG;
    let particle = FIRST_USER_TAG + 1;
    let aux = FIRST_USER_TAG + 2;

    for (surface, lemma, reading, tag, cost) in [
        ("私", "私", "ワタシ", noun, 300),
        ("は", "は", "ハ", particle, 200),
        ("東京大学", "東京大学", "トウキョウダイガク", noun, 100),
        ("学生", "学生", "ガクセイ", noun, 250),
        ("です", "です", "デス", aux, 200),
    ] {
        model.add_entry(surface, lemma, reading, tag, cost).unwrap();
    }

    let mut stream = model.stream();
    for chunk in ["私", "は東", "京", "大学", "の学", "生で", "す"] {
        let delta = stream.append(chunk);
        println!(
            "chunk={chunk:?} retract={} push={:?} tail={}B",
            delta.retract,
            delta
                .push
                .iter()
                .map(|t| t.surface.as_ref())
                .collect::<Vec<_>>(),
            stream.tail_bytes()
        );
    }
    let delta = stream.finish();
    println!(
        "finish retract={} push={:?}",
        delta.retract,
        delta
            .push
            .iter()
            .map(|t| t.surface.as_ref())
            .collect::<Vec<_>>()
    );
    println!(
        "tokens={:?}",
        stream
            .tokens()
            .iter()
            .map(|t| t.surface.as_ref())
            .collect::<Vec<_>>()
    );
}
