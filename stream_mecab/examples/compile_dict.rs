use std::{env, ffi::OsString, fs, process};
use stream_mecab::Model;

fn usage() -> ! {
    eprintln!("usage: compile_dict <dictionary.tsv> [transitions.tsv] <dictionary.smd1>");
    process::exit(2);
}

fn read_text(path: &OsString, kind: &str) -> String {
    fs::read_to_string(path).unwrap_or_else(|error| {
        eprintln!("failed to read {kind} {path:?}: {error}");
        process::exit(1);
    })
}

fn main() {
    let args: Vec<OsString> = env::args_os().skip(1).collect();
    let (dictionary, transitions, output) = match args.as_slice() {
        [dictionary, output] => (dictionary, None, output),
        [dictionary, transitions, output] => (dictionary, Some(transitions), output),
        _ => usage(),
    };

    let dictionary_text = read_text(dictionary, "dictionary");
    let mut model = Model::new();
    let entries = model.add_tsv(&dictionary_text).unwrap_or_else(|error| {
        eprintln!("invalid dictionary {dictionary:?}: {error}");
        process::exit(1);
    });
    let transition_count = transitions.map_or(0, |path| {
        let text = read_text(path, "transitions");
        model.add_transition_tsv(&text).unwrap_or_else(|error| {
            eprintln!("invalid transitions {path:?}: {error}");
            process::exit(1);
        })
    });

    let compiled = model.to_compiled();
    fs::write(output, &compiled).unwrap_or_else(|error| {
        eprintln!("failed to write {output:?}: {error}");
        process::exit(1);
    });
    eprintln!(
        "compiled {entries} entries and {transition_count} transitions into {} bytes",
        compiled.len()
    );
}
