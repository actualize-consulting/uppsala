use std::hint::black_box;
use std::time::Instant;

use uppsala::{parse, XPath2Evaluator};

fn bench(name: &str, iterations: usize, mut f: impl FnMut()) {
    for _ in 0..10 {
        f();
    }

    let start = Instant::now();
    for _ in 0..iterations {
        f();
    }
    let elapsed = start.elapsed();
    let nanos = elapsed.as_nanos() / iterations as u128;
    println!("{name}: {nanos} ns/iter");
}

fn main() {
    let xml = r#"<library>
  <book category="fiction"><title>Dune</title><price>10.0</price></book>
  <book category="science"><title>Cosmos</title><price>12.5</price></book>
  <book category="fiction"><title>Foundation</title><price>8.0</price></book>
</library>"#;
    let doc = parse(xml).unwrap();
    let eval = XPath2Evaluator::new();
    let root = doc.root();

    bench("xpath2_path", 10_000, || {
        black_box(eval.evaluate(&doc, root, "//book/title").unwrap());
    });
    bench("xpath2_predicate", 10_000, || {
        black_box(
            eval.evaluate(&doc, root, "//book[@category = 'fiction']/title")
                .unwrap(),
        );
    });
}
