use std::{
    env, fs,
    hint::black_box,
    path::{Path, PathBuf},
    time::{Duration, Instant},
};

const DEFAULT_SAMPLES: usize = 31;
const WARMUPS: usize = 5;

struct Row {
    name: String,
    bytes: usize,
    uppsala_ns: Duration,
    uppsala_no_ns: Duration,
    roxmltree: Duration,
}

fn bench_one<F, T>(mut f: F, samples: usize) -> Vec<Duration>
where
    F: FnMut() -> T,
{
    for _ in 0..WARMUPS {
        black_box(f());
    }

    let mut times = Vec::with_capacity(samples);
    for _ in 0..samples {
        let start = Instant::now();
        black_box(f());
        times.push(start.elapsed());
    }
    times
}

fn median(times: &mut [Duration]) -> Duration {
    times.sort_unstable();
    times[times.len() / 2]
}

fn bench_text(name: impl Into<String>, text: &str, samples: usize) -> Row {
    let parser_ns = uppsala::Parser::new();
    let parser_no_ns = uppsala::Parser::with_namespace_aware(false);

    let mut uppsala_ns = bench_one(|| parser_ns.parse(text).unwrap(), samples);
    let mut uppsala_no_ns = bench_one(|| parser_no_ns.parse(text).unwrap(), samples);
    let mut roxmltree = bench_one(
        || {
            roxmltree::Document::parse_with_options(
                text,
                roxmltree::ParsingOptions {
                    allow_dtd: true,
                    ..Default::default()
                },
            )
            .unwrap()
        },
        samples,
    );

    Row {
        name: name.into(),
        bytes: text.len(),
        uppsala_ns: median(&mut uppsala_ns),
        uppsala_no_ns: median(&mut uppsala_no_ns),
        roxmltree: median(&mut roxmltree),
    }
}

fn print_header() {
    println!("file\tbytes\tuppsala_ns_us\tuppsala_no_ns_us\troxmltree_us\tratio_ns\tratio_no_ns");
}

fn print_row(row: &Row) {
    let u_ns = micros(row.uppsala_ns);
    let u_no_ns = micros(row.uppsala_no_ns);
    let r = micros(row.roxmltree);
    println!(
        "{}\t{}\t{:.3}\t{:.3}\t{:.3}\t{:.3}\t{:.3}",
        row.name,
        row.bytes,
        u_ns,
        u_no_ns,
        r,
        row.roxmltree.as_secs_f64() / row.uppsala_ns.as_secs_f64(),
        row.roxmltree.as_secs_f64() / row.uppsala_no_ns.as_secs_f64()
    );
}

fn micros(duration: Duration) -> f64 {
    duration.as_nanos() as f64 / 1000.0
}

fn run_file(path: &Path, samples: usize) {
    let text = fs::read_to_string(path).unwrap();
    print_header();
    print_row(&bench_text(path.display().to_string(), &text, samples));
}

fn run_suite(dir: &Path, samples: usize) {
    let files = [
        "fonts.conf",
        "medium.svg",
        "large.plist",
        "huge.xml",
        "gigantic.svg",
        "cdata.xml",
        "text.xml",
        "attributes.xml",
    ];

    print_header();
    for file in files {
        let path = dir.join(file);
        let text = fs::read_to_string(&path).unwrap();
        print_row(&bench_text(file, &text, samples));
    }
}

fn run_saml(samples: usize) {
    let cases = [
        ("saml-small", saml_fixture(3, 1, 8)),
        ("saml-medium", saml_fixture(16, 2, 32)),
        ("saml-large", saml_fixture(64, 4, 96)),
    ];

    print_header();
    for (name, text) in cases {
        print_row(&bench_text(name, &text, samples));
    }
}

fn saml_fixture(attr_count: usize, statement_count: usize, cert_repeats: usize) -> String {
    let attrs = (0..attr_count)
        .map(|i| {
            format!(
                r#"
        <saml:Attribute Name="urn:example:attr:{i}" NameFormat="urn:oasis:names:tc:SAML:2.0:attrname-format:uri">
          <saml:AttributeValue xmlns:xsi="http://www.w3.org/2001/XMLSchema-instance" xsi:type="xs:string">value-{i}</saml:AttributeValue>
        </saml:Attribute>"#
            )
        })
        .collect::<String>();

    let statements = (0..statement_count)
        .map(|i| {
            format!(
                r#"
      <saml:AuthnStatement AuthnInstant="2026-06-29T12:{:02}:00Z" SessionIndex="_{i}">
        <saml:AuthnContext><saml:AuthnContextClassRef>urn:oasis:names:tc:SAML:2.0:ac:classes:PasswordProtectedTransport</saml:AuthnContextClassRef></saml:AuthnContext>
      </saml:AuthnStatement>"#,
                i % 60
            )
        })
        .collect::<String>();

    let cert = "MIIC8DCCAdigAwIBAgIJAO0123456789abcdef".repeat(cert_repeats);

    format!(
        r##"<samlp:Response xmlns:samlp="urn:oasis:names:tc:SAML:2.0:protocol" xmlns:saml="urn:oasis:names:tc:SAML:2.0:assertion" xmlns:ds="http://www.w3.org/2000/09/xmldsig#" xmlns:xs="http://www.w3.org/2001/XMLSchema" ID="_response" Version="2.0" IssueInstant="2026-06-29T12:00:00Z" Destination="https://sp.example.com/acs">
  <saml:Issuer>https://idp.example.com/metadata</saml:Issuer>
  <ds:Signature>
    <ds:SignedInfo>
      <ds:CanonicalizationMethod Algorithm="http://www.w3.org/2001/10/xml-exc-c14n#"/>
      <ds:SignatureMethod Algorithm="http://www.w3.org/2001/04/xmldsig-more#rsa-sha256"/>
      <ds:Reference URI="#_response"><ds:DigestMethod Algorithm="http://www.w3.org/2001/04/xmlenc#sha256"/><ds:DigestValue>abc123digest</ds:DigestValue></ds:Reference>
    </ds:SignedInfo>
    <ds:SignatureValue>{cert}</ds:SignatureValue>
    <ds:KeyInfo><ds:X509Data><ds:X509Certificate>{cert}</ds:X509Certificate></ds:X509Data></ds:KeyInfo>
  </ds:Signature>
  <saml:Assertion ID="_assertion" Version="2.0" IssueInstant="2026-06-29T12:00:00Z">
    <saml:Issuer>https://idp.example.com/metadata</saml:Issuer>
    <saml:Subject><saml:NameID Format="urn:oasis:names:tc:SAML:1.1:nameid-format:emailAddress">user@example.com</saml:NameID><saml:SubjectConfirmation Method="urn:oasis:names:tc:SAML:2.0:cm:bearer"><saml:SubjectConfirmationData NotOnOrAfter="2026-06-29T12:05:00Z" Recipient="https://sp.example.com/acs"/></saml:SubjectConfirmation></saml:Subject>
    <saml:Conditions NotBefore="2026-06-29T12:00:00Z" NotOnOrAfter="2026-06-29T12:05:00Z"><saml:AudienceRestriction><saml:Audience>https://sp.example.com/metadata</saml:Audience></saml:AudienceRestriction></saml:Conditions>{statements}
    <saml:AttributeStatement>{attrs}
    </saml:AttributeStatement>
  </saml:Assertion>
</samlp:Response>"##
    )
}

fn parse_samples(args: &[String], index: usize) -> usize {
    args.get(index)
        .and_then(|s| s.parse().ok())
        .unwrap_or(DEFAULT_SAMPLES)
}

fn usage() -> ! {
    eprintln!(
        "usage:
  uppsala-performance-harness file <xml-file> [samples]
  uppsala-performance-harness suite <roxmltree-benches-dir> [samples]
  uppsala-performance-harness saml [samples]"
    );
    std::process::exit(2);
}

fn main() {
    let args = env::args().collect::<Vec<_>>();
    match args.get(1).map(String::as_str) {
        Some("file") => {
            let path = args.get(2).map(PathBuf::from).unwrap_or_else(|| usage());
            run_file(&path, parse_samples(&args, 3));
        }
        Some("suite") => {
            let dir = args.get(2).map(PathBuf::from).unwrap_or_else(|| usage());
            run_suite(&dir, parse_samples(&args, 3));
        }
        Some("saml") => run_saml(parse_samples(&args, 2)),
        _ => usage(),
    }
}
