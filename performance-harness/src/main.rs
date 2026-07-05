use std::{
    env, fs,
    hint::black_box,
    os::raw::{c_char, c_int},
    path::{Path, PathBuf},
    ptr,
    sync::Once,
    time::{Duration, Instant},
};

const DEFAULT_SAMPLES: usize = 31;
const WARMUPS: usize = 5;
const XML_PARSE_NONET: c_int = 1 << 11;
const XML_PARSE_COMPACT: c_int = 1 << 16;
const LIBXML2_PARSE_OPTIONS: c_int = XML_PARSE_NONET | XML_PARSE_COMPACT;

#[repr(C)]
struct XmlDoc {
    _private: [u8; 0],
}

type XmlDocPtr = *mut XmlDoc;

extern "C" {
    fn xmlInitParser();
    fn xmlCleanupParser();
    fn xmlReadMemory(
        buffer: *const c_char,
        size: c_int,
        url: *const c_char,
        encoding: *const c_char,
        options: c_int,
    ) -> XmlDocPtr;
    fn xmlFreeDoc(cur: XmlDocPtr);
}

static LIBXML2_INIT: Once = Once::new();

fn init_libxml2() {
    LIBXML2_INIT.call_once(|| unsafe {
        xmlInitParser();
    });
}

struct Libxml2Cleanup;

impl Drop for Libxml2Cleanup {
    fn drop(&mut self) {
        unsafe {
            xmlCleanupParser();
        }
    }
}

struct Row {
    name: String,
    bytes: usize,
    uppsala_ns: Duration,
    uppsala_no_ns: Duration,
    uppsala_pull_scan: Duration,
    uppsala_pull_dom: Duration,
    libxml2: Duration,
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
    let mut uppsala_pull_scan = bench_one(|| parse_pull_scan(text), samples);
    let mut uppsala_pull_dom = bench_one(|| parse_pull_dom(text), samples);
    let mut libxml2 = bench_one(|| parse_libxml2(text), samples);

    Row {
        name: name.into(),
        bytes: text.len(),
        uppsala_ns: median(&mut uppsala_ns),
        uppsala_no_ns: median(&mut uppsala_no_ns),
        uppsala_pull_scan: median(&mut uppsala_pull_scan),
        uppsala_pull_dom: median(&mut uppsala_pull_dom),
        libxml2: median(&mut libxml2),
    }
}

fn print_header() {
    println!("file\tbytes\tuppsala_ns_us\tuppsala_no_ns_us\tuppsala_pull_scan_us\tuppsala_pull_dom_us\tlibxml2_us\tratio_ns\tratio_no_ns\tratio_pull_scan\tratio_pull_dom");
}

fn print_row(row: &Row) {
    let u_ns = micros(row.uppsala_ns);
    let u_no_ns = micros(row.uppsala_no_ns);
    let u_pull_scan = micros(row.uppsala_pull_scan);
    let u_pull_dom = micros(row.uppsala_pull_dom);
    let l = micros(row.libxml2);
    println!(
        "{}\t{}\t{:.3}\t{:.3}\t{:.3}\t{:.3}\t{:.3}\t{:.3}\t{:.3}\t{:.3}\t{:.3}",
        row.name,
        row.bytes,
        u_ns,
        u_no_ns,
        u_pull_scan,
        u_pull_dom,
        l,
        row.libxml2.as_secs_f64() / row.uppsala_ns.as_secs_f64(),
        row.libxml2.as_secs_f64() / row.uppsala_no_ns.as_secs_f64(),
        row.libxml2.as_secs_f64() / row.uppsala_pull_scan.as_secs_f64(),
        row.libxml2.as_secs_f64() / row.uppsala_pull_dom.as_secs_f64()
    );
}

fn micros(duration: Duration) -> f64 {
    duration.as_nanos() as f64 / 1000.0
}

fn format_duration(duration: Duration) -> String {
    let us = micros(duration);
    if us >= 1000.0 {
        format!("{:.3} ms", us / 1000.0)
    } else {
        format!("{us:.3} us")
    }
}

fn format_size(bytes: usize) -> String {
    if bytes >= 1024 * 1024 {
        format!("{:.2} MB", bytes as f64 / (1024.0 * 1024.0))
    } else if bytes >= 1024 {
        format!("{:.1} KB", bytes as f64 / 1024.0)
    } else {
        format!("{bytes} B")
    }
}

fn parse_libxml2(text: &str) {
    init_libxml2();
    let size: c_int = text.len().try_into().expect("input too large for libxml2");
    let doc = unsafe {
        xmlReadMemory(
            text.as_ptr() as *const c_char,
            size,
            ptr::null(),
            ptr::null(),
            LIBXML2_PARSE_OPTIONS,
        )
    };
    assert!(!doc.is_null(), "libxml2 parse failed");
    black_box(doc);
    unsafe {
        xmlFreeDoc(doc);
    }
}

fn parse_pull_scan(text: &str) {
    let mut parser = uppsala::PullParser::new(text);
    while let Some(event) = parser.next_event().unwrap() {
        black_box(event);
    }
}

fn parse_pull_dom(text: &str) {
    let doc = uppsala::pull::document_from_pull(text, uppsala::PullParser::new(text)).unwrap();
    black_box(doc);
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

fn run_libxml2_report(samples: usize) {
    let mut rows = Vec::new();
    let libxml2_dir = env::var_os("LIBXML2_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("../libxml2"));

    let saml_cases = [
        ("SAML small".to_string(), saml_fixture(3, 1, 8)),
        ("SAML medium".to_string(), saml_fixture(16, 2, 32)),
        ("SAML large".to_string(), saml_fixture(64, 4, 96)),
    ];
    for (name, text) in saml_cases {
        rows.push(bench_text(name, &text, samples));
    }

    let generated_cases = [
        (
            "SAML metadata aggregate".to_string(),
            saml_metadata_fixture(600, 4),
        ),
        ("Atom feed archive".to_string(), atom_feed_fixture(1_500, 2)),
        (
            "SOAP invoice batch".to_string(),
            soap_invoice_batch_fixture(350, 8),
        ),
    ];
    for (name, text) in generated_cases {
        rows.push(bench_text(name, &text, samples));
    }

    let file_cases = [
        (
            "pyFF sample metadata",
            PathBuf::from("test-data/pyff-xslt/sample-metadata.xml"),
        ),
        (
            "libxml2 nvdcve_0.xml",
            libxml2_dir.join("test/schemas/nvdcve_0.xml"),
        ),
        (
            "libxml2 comps_0.xml",
            libxml2_dir.join("test/relaxng/comps_0.xml"),
        ),
    ];
    for (name, path) in file_cases {
        let text = fs::read_to_string(&path).unwrap_or_else(|err| {
            panic!("failed to read benchmark fixture {}: {err}", path.display());
        });
        rows.push(bench_text(name, &text, samples));
    }

    println!("| Input | Size | Uppsala ns | Uppsala no-ns | Pull scan | Pull DOM | libxml2 | Ratio ns | Ratio no-ns | Ratio pull scan | Ratio pull DOM |");
    println!("|---|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|");
    for row in rows {
        println!(
            "| {} | {} | {} | {} | {} | {} | {} | {:.2}x | {:.2}x | {:.2}x | {:.2}x |",
            row.name,
            format_size(row.bytes),
            format_duration(row.uppsala_ns),
            format_duration(row.uppsala_no_ns),
            format_duration(row.uppsala_pull_scan),
            format_duration(row.uppsala_pull_dom),
            format_duration(row.libxml2),
            row.libxml2.as_secs_f64() / row.uppsala_ns.as_secs_f64(),
            row.libxml2.as_secs_f64() / row.uppsala_no_ns.as_secs_f64(),
            row.libxml2.as_secs_f64() / row.uppsala_pull_scan.as_secs_f64(),
            row.libxml2.as_secs_f64() / row.uppsala_pull_dom.as_secs_f64(),
        );
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

fn saml_metadata_fixture(entity_count: usize, cert_repeats: usize) -> String {
    let cert = "MIIC8DCCAdigAwIBAgIJAO0123456789abcdef".repeat(cert_repeats);
    let mut xml = String::with_capacity(entity_count * 1300);
    xml.push_str(r#"<md:EntitiesDescriptor xmlns:md="urn:oasis:names:tc:SAML:2.0:metadata" xmlns:ds="http://www.w3.org/2000/09/xmldsig#" xmlns:saml="urn:oasis:names:tc:SAML:2.0:assertion" Name="urn:example:federation">"#);
    for i in 0..entity_count {
        xml.push_str(&format!(
            r#"<md:EntityDescriptor entityID="https://sp{i}.example.org/metadata">
  <md:SPSSODescriptor AuthnRequestsSigned="true" WantAssertionsSigned="true" protocolSupportEnumeration="urn:oasis:names:tc:SAML:2.0:protocol">
    <md:KeyDescriptor use="signing"><ds:KeyInfo><ds:X509Data><ds:X509Certificate>{cert}</ds:X509Certificate></ds:X509Data></ds:KeyInfo></md:KeyDescriptor>
    <md:AssertionConsumerService Binding="urn:oasis:names:tc:SAML:2.0:bindings:HTTP-POST" Location="https://sp{i}.example.org/saml/acs" index="{i}"/>
    <md:AttributeConsumingService index="{i}"><md:ServiceName xml:lang="en">Example Service {i}</md:ServiceName><md:RequestedAttribute Name="urn:example:attribute:eduPersonPrincipalName" isRequired="true"/></md:AttributeConsumingService>
  </md:SPSSODescriptor>
  <md:Organization><md:OrganizationName xml:lang="en">Example Org {i}</md:OrganizationName><md:OrganizationURL xml:lang="en">https://sp{i}.example.org/</md:OrganizationURL></md:Organization>
</md:EntityDescriptor>"#
        ));
    }
    xml.push_str("</md:EntitiesDescriptor>");
    xml
}

fn atom_feed_fixture(entry_count: usize, body_repeats: usize) -> String {
    let body = "This release note describes parser behavior, deployment status, and operational metadata. ".repeat(body_repeats);
    let mut xml = String::with_capacity(entry_count * (body.len() + 450));
    xml.push_str(r#"<feed xmlns="http://www.w3.org/2005/Atom" xmlns:app="urn:example:app"><title>Operations Feed</title><id>urn:example:feed</id><updated>2026-07-04T12:00:00Z</updated>"#);
    for i in 0..entry_count {
        xml.push_str(&format!(
            r#"<entry>
  <title>Deployment event {i}</title>
  <id>urn:example:event:{i}</id>
  <updated>2026-07-04T12:{:02}:00Z</updated>
  <author><name>Platform Team</name><email>platform@example.org</email></author>
  <category term="deployment"/><category term="region-{}"/>
  <link rel="alternate" href="https://status.example.org/events/{i}"/>
  <app:severity>{}</app:severity>
  <summary>{body}</summary>
</entry>"#,
            i % 60,
            i % 8,
            if i % 17 == 0 { "warning" } else { "info" }
        ));
    }
    xml.push_str("</feed>");
    xml
}

fn soap_invoice_batch_fixture(invoice_count: usize, lines_per_invoice: usize) -> String {
    let mut xml = String::with_capacity(invoice_count * lines_per_invoice * 260);
    xml.push_str(r#"<soap:Envelope xmlns:soap="http://schemas.xmlsoap.org/soap/envelope/" xmlns:inv="urn:example:invoice" xmlns:xs="http://www.w3.org/2001/XMLSchema"><soap:Header><inv:Batch id="batch-2026-07-04" region="eu-north"/></soap:Header><soap:Body><inv:Invoices>"#);
    for invoice in 0..invoice_count {
        xml.push_str(&format!(
            r#"<inv:Invoice id="INV-{invoice:06}" currency="EUR" issued="2026-07-04">
  <inv:Customer id="CUST-{invoice:06}"><inv:Name>Example Customer {invoice}</inv:Name><inv:Country>SE</inv:Country></inv:Customer>
  <inv:Lines>"#
        ));
        for line in 0..lines_per_invoice {
            xml.push_str(&format!(
                r#"<inv:Line number="{line}"><inv:Sku>SKU-{invoice:06}-{line:03}</inv:Sku><inv:Description>Managed service subscription tier {}</inv:Description><inv:Quantity>{}</inv:Quantity><inv:UnitPrice>{}.95</inv:UnitPrice><inv:TaxRate>25</inv:TaxRate></inv:Line>"#,
                line % 5,
                (line % 9) + 1,
                10 + (invoice + line) % 90
            ));
        }
        xml.push_str("</inv:Lines><inv:Total>");
        xml.push_str(&(lines_per_invoice * 42).to_string());
        xml.push_str(".00</inv:Total></inv:Invoice>");
    }
    xml.push_str("</inv:Invoices></soap:Body></soap:Envelope>");
    xml
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
  uppsala-performance-harness suite <benchmark-files-dir> [samples]
  uppsala-performance-harness libxml2-report [samples]
  uppsala-performance-harness saml [samples]"
    );
    std::process::exit(2);
}

fn main() {
    let _cleanup = Libxml2Cleanup;
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
        Some("libxml2-report") => run_libxml2_report(parse_samples(&args, 2)),
        Some("saml") => run_saml(parse_samples(&args, 2)),
        _ => usage(),
    }
}
