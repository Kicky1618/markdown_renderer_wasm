use std::{env, fs, process};
use stream_mecab::Model;

fn main() {
    let mut args = env::args_os().skip(1);
    let Some(input) = args.next() else {
        eprintln!("usage: compile_dict <dictionary.tsv> <dictionary.smd1>");
        process::exit(2);
    };
    let Some(output) = args.next() else {
        eprintln!("usage: compile_dict <dictionary.tsv> <dictionary.smd1>");
        process::exit(2);
    };
    if args.next().is_some() {
        eprintln!("usage: compile_dict <dictionary.tsv> <dictionary.smd1>");
        process::exit(2);
    }
    let text = fs::read_to_string(&input).unwrap_or_else(|error| {
        eprintln!("failed to read {input:?}: {error}");
        process::exit(1);
    });
    let mut model = Model::new();
    let entries = model.add_tsv(&text).unwrap_or_else(|error| {
        eprintln!("invalid dictionary {input:?}: {error}");
        process::exit(1);
    });
    let compiled = model.to_compiled();
    fs::write(&output, &compiled).unwrap_or_else(|error| {
        eprintln!("failed to write {output:?}: {error}");
        process::exit(1);
    });
    eprintln!("compiled {entries} entries into {} bytes", compiled.len());
}
