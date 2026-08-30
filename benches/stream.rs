use std::{hint::black_box, time::Instant};
use streamdown::Parser;

fn throughput(label: &str, appends: usize, bytes: usize, elapsed: std::time::Duration) {
    println!(
        "{label}: {appends} appends / {:.2} MiB in {elapsed:?} ({:.0} appends/s, {:.1} MiB/s)",
        bytes as f64 / 1_048_576.0,
        appends as f64 / elapsed.as_secs_f64(),
        bytes as f64 / 1_048_576.0 / elapsed.as_secs_f64(),
    );
}

fn main() {
    let tokens = 100_000;
    let mut parser = Parser::new();
    let start = Instant::now();
    let mut bytes = 0;
    for i in 0..tokens {
        let chunk = if i % 17 == 0 {
            " **fast**\n\n"
        } else {
            "token "
        };
        bytes += chunk.len();
        black_box(parser.append(chunk));
    }
    throughput("paragraph stream", tokens, bytes, start.elapsed());

    // LLMs often emit a long paragraph with no blank line for many chunks.
    // This catches accidental O(total_document_size) work per append.
    let long_tokens = 20_000;
    let chunk = "token ";
    let mut parser = Parser::new();
    let start = Instant::now();
    for _ in 0..long_tokens {
        black_box(parser.append(chunk));
    }
    throughput(
        "long live paragraph",
        long_tokens,
        long_tokens * chunk.len(),
        start.elapsed(),
    );

    let mut parser = Parser::new();
    parser.append("```text\n");
    let line = "0123456789abcdef0123456789abcdef\n";
    let start = Instant::now();
    for _ in 0..tokens {
        black_box(parser.append(line));
    }
    throughput(
        "open code stream",
        tokens,
        tokens * line.len(),
        start.elapsed(),
    );

    let mut parser = Parser::new();
    parser.append(":::llm tool name=bulk id=bench\n");
    let payload = "{\"token\":\"0123456789abcdef\"}\n";
    let start = Instant::now();
    for _ in 0..tokens {
        black_box(parser.append(payload));
    }
    let elapsed = start.elapsed();
    println!(
        "llm semantic stream: {} MiB in {elapsed:?} ({:.0} MiB/s)",
        tokens * payload.len() / 1_048_576,
        (tokens * payload.len()) as f64 / 1_048_576.0 / elapsed.as_secs_f64()
    );
}
