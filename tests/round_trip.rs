use xancode::Codec;

fn round_trip<T: Codec + std::fmt::Debug + PartialEq>(value: T)
where
    T::Error: std::fmt::Debug,
{
    let encoded = value.encode();
    let header = u32::from_be_bytes(encoded[0..4].try_into().unwrap()) as usize;
    assert_eq!(
        header,
        encoded.len() - 4,
        "header length mismatch: header={}, payload={}",
        header,
        encoded.len() - 4,
    );
    let decoded = T::decode(&encoded).unwrap();
    assert_eq!(value, decoded);
}

#[derive(Codec, Debug, PartialEq)]
struct Primitives {
    a: u8,
    b: u16,
    c: u32,
    d: u64,
    e: u128,
    f: i8,
    g: i16,
    h: i32,
    i: i64,
    j: i128,
}

#[test]
fn primitives_roundtrip() {
    round_trip(Primitives {
        a: 0xAB,
        b: 0xABCD,
        c: 0xDEAD_BEEF,
        d: 0x0123_4567_89AB_CDEF,
        e: u128::MAX / 2,
        f: -1,
        g: -1234,
        h: i32::MIN,
        i: i64::MAX,
        j: i128::MIN + 1,
    });
}

#[test]
fn primitives_zero() {
    round_trip(Primitives {
        a: 0,
        b: 0,
        c: 0,
        d: 0,
        e: 0,
        f: 0,
        g: 0,
        h: 0,
        i: 0,
        j: 0,
    });
}

#[derive(Codec, Debug, PartialEq)]
struct Bools {
    t: bool,
    f: bool,
}

#[test]
fn bool_roundtrip() {
    round_trip(Bools { t: true, f: false });
    round_trip(Bools { t: false, f: true });
}

#[test]
fn invalid_bool_fails() {
    let mut buf: Vec<u8> = Vec::new();
    buf.extend_from_slice(&0u32.to_be_bytes());
    buf.push(7u8); // invalid bool
    buf.push(0u8);
    let len = (buf.len() - 4) as u32;
    buf[0..4].copy_from_slice(&len.to_be_bytes());
    let bytes = xancode::Bytes::from(buf);
    assert!(Bools::decode(&bytes).is_err());
}

#[derive(Codec, Debug, PartialEq)]
struct Floats {
    a: f32,
    b: f64,
}

#[test]
fn float_roundtrip() {
    round_trip(Floats {
        a: 3.14159,
        b: -2.718281828459045,
    });
}

#[test]
fn float_specials() {
    round_trip(Floats { a: 0.0, b: -0.0 });
    round_trip(Floats {
        a: f32::INFINITY,
        b: f64::NEG_INFINITY,
    });
    round_trip(Floats {
        a: f32::MIN_POSITIVE,
        b: f64::EPSILON,
    });
}

#[derive(Codec, Debug, PartialEq)]
struct Mixed {
    flag: bool,
    weight: f64,
    tags: Vec<bool>,
    maybe_pi: Option<f32>,
}

#[test]
fn mixed_bool_float() {
    round_trip(Mixed {
        flag: true,
        weight: 12.5,
        tags: vec![true, false, true, true],
        maybe_pi: Some(std::f32::consts::PI),
    });
    round_trip(Mixed {
        flag: false,
        weight: 0.0,
        tags: vec![],
        maybe_pi: None,
    });
}

#[derive(Codec, Debug, PartialEq)]
struct Strings {
    s: String,
}

#[test]
fn string_ascii() {
    round_trip(Strings {
        s: "hello, world!".to_string(),
    });
}

#[test]
fn string_unicode() {
    round_trip(Strings {
        s: "안녕 🦀 \u{1F600}".to_string(),
    });
}

#[test]
fn string_empty() {
    round_trip(Strings { s: String::new() });
}

#[derive(Codec, Debug, PartialEq)]
struct Vectors {
    nums: Vec<u32>,
    strs: Vec<String>,
}

#[test]
fn vec_filled() {
    round_trip(Vectors {
        nums: vec![1, 2, 3, u32::MAX, 0],
        strs: vec!["foo".into(), "bar".into(), String::new(), "baz".into()],
    });
}

#[test]
fn vec_empty() {
    round_trip(Vectors {
        nums: vec![],
        strs: vec![],
    });
}

#[derive(Codec, Debug, PartialEq)]
struct Options {
    a: Option<u32>,
    b: Option<String>,
}

#[test]
fn option_some() {
    round_trip(Options {
        a: Some(42),
        b: Some("yes".into()),
    });
}

#[test]
fn option_none() {
    round_trip(Options { a: None, b: None });
}

#[test]
fn option_mixed() {
    round_trip(Options {
        a: None,
        b: Some("only b".into()),
    });
}

#[derive(Codec, Debug, PartialEq)]
struct Inner {
    x: u32,
    label: String,
}

#[derive(Codec, Debug, PartialEq)]
struct Outer {
    id: u64,
    inner: Inner,
    tail: String,
}

#[test]
fn nested_struct() {
    round_trip(Outer {
        id: 99,
        inner: Inner {
            x: 7,
            label: "inside".into(),
        },
        tail: "after".into(),
    });
}

#[derive(Codec, Debug, PartialEq)]
struct DoubleNest {
    outer: Outer,
}

#[test]
fn nested_two_levels() {
    round_trip(DoubleNest {
        outer: Outer {
            id: 1,
            inner: Inner {
                x: 2,
                label: "deep".into(),
            },
            tail: "end".into(),
        },
    });
}

#[derive(Codec, Debug, PartialEq)]
struct Combos {
    vec_of_opts: Vec<Option<u32>>,
    opt_of_vec: Option<Vec<String>>,
    vec_of_nested: Vec<Inner>,
    opt_of_nested: Option<Inner>,
    nested_vec: Vec<Vec<u32>>,
}

#[test]
fn combos_filled() {
    round_trip(Combos {
        vec_of_opts: vec![Some(1), None, Some(3), None],
        opt_of_vec: Some(vec!["a".into(), "b".into()]),
        vec_of_nested: vec![
            Inner {
                x: 1,
                label: "one".into(),
            },
            Inner {
                x: 2,
                label: "two".into(),
            },
        ],
        opt_of_nested: Some(Inner {
            x: 99,
            label: "deep".into(),
        }),
        nested_vec: vec![vec![1, 2], vec![], vec![3, 4, 5]],
    });
}

#[test]
fn combos_emptyish() {
    round_trip(Combos {
        vec_of_opts: vec![None, None],
        opt_of_vec: None,
        vec_of_nested: vec![],
        opt_of_nested: None,
        nested_vec: vec![],
    });
}

#[test]
fn invalid_option_tag_fails() {
    // Hand-craft a payload with an invalid Option tag (2) and ensure decode errors.
    let mut buf: Vec<u8> = Vec::new();
    buf.extend_from_slice(&0u32.to_be_bytes()); // header placeholder
    buf.push(2u8); // bogus Option<u32> tag
    buf.push(2u8); // bogus Option<String> tag (won't be reached)
    let len = (buf.len() - 4) as u32;
    buf[0..4].copy_from_slice(&len.to_be_bytes());
    let bytes = xancode::Bytes::from(buf);
    let result = Options::decode(&bytes);
    assert!(
        result.is_err(),
        "expected decode error for invalid Option tag"
    );
}

#[test]
fn truncated_payload_fails() {
    // Header claims more bytes than actually present.
    let mut buf: Vec<u8> = Vec::new();
    buf.extend_from_slice(&100u32.to_be_bytes()); // claim 100 bytes of payload
    buf.push(0u8); // only 1 byte of actual payload
    let bytes = xancode::Bytes::from(buf);
    let result = Primitives::decode(&bytes);
    assert!(
        result.is_err(),
        "expected decode error for truncated payload"
    );
}
