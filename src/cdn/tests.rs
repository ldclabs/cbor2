use alloc::{
    format,
    string::{String, ToString},
    vec::Vec,
};

use crate::value::Value;

use super::*;

fn hex(input: &str) -> String {
    let bytes = cdn_to_vec(input).unwrap();
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

fn bytes(input: &str) -> Vec<u8> {
    cdn_to_vec(input).unwrap()
}

fn assert_cdn_eq(left: &str, right: &str) {
    assert_eq!(bytes(left), bytes(right), "{left} != {right}");
}

fn assert_all_eq(inputs: &[&str]) {
    let Some((first, rest)) = inputs.split_first() else {
        return;
    };
    let expected = bytes(first);
    for input in rest {
        assert_eq!(bytes(input), expected, "{input} != {first}");
    }
}

fn assert_hex(input: &str, expected: &str) {
    assert_eq!(hex(input), expected, "{input}");
}

#[test]
fn parses_json_superset_core_items() {
    assert_eq!(hex("0"), "00");
    assert_eq!(hex("-0"), "00");
    assert_eq!(hex("+0x1267"), "191267");
    assert_eq!(hex("0o11147"), "191267");
    assert_eq!(hex("0b1001001100111"), "191267");
    assert_eq!(hex("-18446744073709551617"), "c349010000000000000000");
    assert_eq!(hex("1.5"), "f93e00");
    assert_eq!(hex("0x1.8p0"), "f93e00");
    assert_eq!(hex("0x18p-4"), "f93e00");
    assert_eq!(hex("Infinity"), "f97c00");
    assert_eq!(hex("-Infinity"), "f9fc00");
    assert_eq!(hex("NaN"), "f97e00");
    assert_eq!(hex("false"), "f4");
    assert_eq!(hex("undefined"), "f7");
    assert_eq!(hex("simple(23)_i"), "f7");
    assert_eq!(hex("simple(59)"), "f83b");
}

#[test]
fn parses_strings_comments_and_containers() {
    assert_eq!(
        hex(r#""D\u{6f}mino's \uD83C\uDC73""#),
        "6d446f6d696e6f277320f09f81b3"
    );
    assert_eq!(hex(r#"'hello world'"#), "4b68656c6c6f20776f726c64");
    assert_eq!(hex("`raw\\\\text`"), "697261775c5c74657874");
    assert_eq!(hex("``\n`leading``"), "68606c656164696e67");
    assert_eq!(
        hex(r#"{
                /kty/ 1: 4 # symmetric
                "k": h'66 84 52 3a'
            }"#),
        "a20104616b446684523a"
    );
    assert_eq!(hex("[1 2, 3,]"), "83010203");
    assert_eq!(hex("{_ 1: 2,}"), "bf0102ff");
}

#[test]
fn parses_base_encodings_and_embedded_sequences() {
    assert_eq!(hex("h'68 65 6c /comment/ 6c 6f'"), "4568656c6c6f");
    assert_eq!(hex("b64'SGVsbG8'"), "4548656c6c6f");
    assert_eq!(hex("b64'SGV sbG8='"), "4548656c6c6f");
    assert_eq!(hex("b64'AA=='"), "4100");
    assert_eq!(hex("b64'AAA='"), "420000");
    assert_eq!(hex("<<1, 2>>"), "420102");
    assert_eq!(hex("[<<\"hello\", null>>]"), "81476568656c6c6ff6");
}

#[test]
fn honors_encoding_indicators() {
    assert_eq!(hex("1_i"), "01");
    assert_eq!(hex("1_0"), "1801");
    assert_eq!(hex("1_1"), "190001");
    assert_eq!(hex("0x4711_3"), "1b0000000000004711");
    assert_eq!(hex("'A'_1"), "59000141");
    assert_eq!(hex(r#""A"_1"#), "79000141");
    assert_eq!(hex("[_0 false, true]"), "9802f4f5");
    assert_eq!(hex("{_1 \"bar\": 1}"), "b900016362617201");
    assert_eq!(hex("1_1(4711)"), "d90001191267");
    assert_eq!(hex("1.5_2"), "fa3fc00000");
    assert_eq!(hex("Infinity_3"), "fb7ff0000000000000");
    assert_eq!(hex("''_"), "5fff");
    assert_eq!(hex("\"\"_"), "7fff");
    assert_eq!(hex("unknown'foo'_3"), "d903e78267756e6b6e6f776e8163666f6f");
    assert_eq!(hex("h'4711...0815'_2"), "d9037883424711d90378f6420815");
    assert_eq!(hex("ilbs<<>>_1"), "5fff");
}

#[test]
fn parses_application_extensions() {
    assert_eq!(hex("dt'1969-07-21T02:56:16Z'"), "3a00d80caf");
    assert_eq!(hex("dt'1969-07-21t02:56:16z'"), "3a00d80caf");
    assert_eq!(hex("dt'2016-12-31T23:59:60Z'"), "1a58684680");
    assert_eq!(hex("dt'2017-01-01T00:59:60+01:00'"), "1a58684680");
    assert_eq!(hex("DT'1969-07-21T02:56:16Z'"), "c13a00d80caf");
    assert_eq!(hex("ip'192.0.2.42'"), "44c000022a");
    assert_eq!(hex("IP'192.0.2.42'"), "d83444c000022a");
    assert_eq!(hex("IP'192.0.2.0/24'"), "d83482181843c00002");
    assert_eq!(
        hex("IP'2001:db8::42'"),
        "d8365020010db8000000000000000000000042"
    );
    assert_eq!(hex("IP'2001:db8::/64'"), "d8368218404420010db8");
    assert_eq!(
        hex(r#"t1<<"Hello", h'20', "world">>"#),
        "6b48656c6c6f20776f726c64"
    );
    assert_eq!(
        hex(r#"b1<<"Hello", h'20', "world">>"#),
        "4b48656c6c6f20776f726c64"
    );
    assert_eq!(
        hex(r#"ilbs<<'Hello '_0, 'world'>>"#),
        "5f580648656c6c6f2045776f726c64ff"
    );
    assert_eq!(
        hex(r#"ilbs<<"Hello world">>"#),
        "5f4b48656c6c6f20776f726c64ff"
    );
    assert_eq!(
        hex(r#"ilts<<'Hello '_0, h'776f726c64'>>"#),
        "7f780648656c6c6f2065776f726c64ff"
    );
    assert_eq!(hex("float'fe00'"), "f9fe00");
    assert_eq!(hex("float'fe00'_2"), "faffc00000");
}

#[cfg(feature = "cdn")]
#[test]
fn parses_hash_and_cri_extensions() {
    assert_eq!(
        hex("hash'foo'"),
        "58202c26b46b68ffc68ff99b453c1d30413413422d706483bfa0f98a5e886266e7ae"
    );
    assert_eq!(
        hex(r#"hash<<'foo', "SHA-512">>"#),
        concat!(
            "5840",
            "f7fbba6e0636f890e56fbbf3283e524c",
            "6fa3204ae298382d624741d0dc663832",
            "6e282c41be5e4254d8820772c5518a2c",
            "5a8c0c7f7eda19594a7eb539453e1ed7"
        )
    );
    assert_eq!(
        hex("cri'https://example.com/bottarga/shaved'"),
        "832382676578616d706c6563636f6d8268626f74746172676166736861766564"
    );
    assert_eq!(
        hex("CRI'https://example.com/bottarga/shaved'"),
        "d863832382676578616d706c6563636f6d8268626f74746172676166736861766564"
    );
    assert_eq!(
        hex("cri'https://\u{4f8b}\u{5b50}.\u{6d4b}\u{8bd5}/\u{8def}\u{5f84}'"),
        "83238266e4be8be5ad9066e6b58be8af958166e8b7afe5be84"
    );
}

#[test]
fn represents_elisions_and_unknown_app_extensions() {
    assert_eq!(hex("..."), "d90378f6");
    assert_eq!(hex("h'4711...0815'"), "d9037883424711d90378f6420815");
    assert_eq!(
        hex(r#"b1<<'Hello', ..., 'world'>>"#),
        "d90378834548656c6c6fd90378f645776f726c64"
    );
    assert_eq!(hex("unknown'foo'"), "d903e78267756e6b6e6f776e8163666f6f");
    assert_eq!(hex("unknown<<1, 2>>"), "d903e78267756e6b6e6f776e820102");
}

#[test]
fn draft26_comment_examples() {
    assert_cdn_eq(
        r#" /grasp-message/ [/M_DISCOVERY/ 1, /session-id/ 10584416,
                    /objective/ [/objective-name/ "opsonize",
                                 /D, N, S/ 7, /loop-count/ 105]]"#,
        r#"[1, 10584416, ["opsonize", 7, 105]]"#,
    );
    assert_cdn_eq(
        r#"{
    /kty/ 1 : 4, # Symmetric
    /alg/ 3 : 5, # HMAC 256-256
     /k/ -1 : h'6684523ab17337f173500e5728c628547cb37df
                e68449c65f885d1b73b49eae1'
   }"#,
        r#"{1: 4, 3: 5, -1:
   h'6684523AB17337F173500E5728C628547CB37DFE68449C65F885D1B73B49EAE1'}"#,
    );
    assert_cdn_eq(
        r#"/* ### MyApp Configuration
    * John Example, 2026-06-09
    */
   {
     // Top-level config for the app
     "appName": "MyApp", // short name shown in UI
     "version": "1.2.0",
     ...: ...
   }"#,
        r#"{
     "appName": "MyApp",
     "version": "1.2.0",
     888(null): 888(null)
   }"#,
    );
    assert_cdn_eq("4 /* HMAC 256/64 */", "4");
    assert_cdn_eq("4 / HMAC 256//64 /", "4");
}

#[test]
fn draft26_encoding_indicator_examples() {
    assert_hex("1_1", "190001");
    assert_hex("0x4711_3", "1b0000000000004711");
    assert_hex("-1_1", "390000");
    assert_hex("'A'_1", "59000141");
    assert_hex(r#""A"_1"#, "79000141");
    assert_hex(r#"[_1 "bar"]"#, "99000163626172");
    assert_hex(r#"{_1 "bar": 1}"#, "b900016362617201");
    assert_hex("1_1(4711)", "d90001191267");
    assert_hex("1.5_2", "fa3fc00000");
    assert_hex("0x4711p+03_3", "fb4101c44000000000");
    assert_hex("1.5_1", "f93e00");
    assert_hex("1.5_3", "fb3ff8000000000000");
    assert_hex("[_ 1, 2]", "9f0102ff");
    assert_hex("[_0 false, true]", "9802f4f5");
}

#[test]
fn draft26_number_examples() {
    assert_all_eq(&["4711", "0x1267", "0o11147", "0b1001001100111"]);
    assert_hex("4711", "191267");
    assert_all_eq(&["1.5", "0.15e1", "15e-1", "0x1.8p0", "0x18p-4"]);
    assert_hex("1.5", "f93e00");
    assert_all_eq(&["0", "+0", "-0"]);
    assert_hex("0", "00");
    assert_all_eq(&["0.0", "+0.0"]);
    assert_hex("0.0", "f90000");
    assert_hex("-0.0", "f98000");
    assert_hex("Infinity", "f97c00");
    assert_hex("-Infinity", "f9fc00");
    assert_hex("NaN", "f97e00");

    assert_hex("1.1", "fb3ff199999999999a");
    assert!(cdn_to_vec("1.1_1").is_err());
    assert!(cdn_to_vec("1.1_2").is_err());
    assert_hex("1.1_3", "fb3ff199999999999a");
    assert_hex("1.5_2", "fa3fc00000");
    assert_hex("Infinity_1", "f97c00");
    assert_hex("Infinity_2", "fa7f800000");
    assert_hex("Infinity_3", "fb7ff0000000000000");
    assert_hex("-Infinity_1", "f9fc00");
    assert_hex("-Infinity_2", "faff800000");
    assert_hex("-Infinity_3", "fbfff0000000000000");
    assert_hex("NaN_1", "f97e00");
    assert_hex("NaN_2", "fa7fc00000");
    assert_hex("NaN_3", "fb7ff8000000000000");

    assert_cdn_eq("987654321098765432310", "2(h'35 8a 75 04 38 f3 80 f5 f6')");
    assert_hex(
        "2_3(h'00 00 00 35 8a 75 04 38 f3 80 f5 f6'_1)",
        "db000000000000000259000c000000358a750438f380f5f6",
    );
}

#[test]
fn draft26_string_examples() {
    assert_all_eq(&[
        r#""D\u{6f}mino's \u{1F073} + \u{2318}""#,
        r#""D\u006Fmino's \uD83C\uDC73 + \u2318""#,
        "\"Domino's 🁳 + ⌘\"",
    ]);
    assert_all_eq(&[r#"'hello world'"#, "h'68656c6c6f20776f726c64'"]);
    assert_hex(r#"'\\'"#, "415c");
    assert_hex(r#"'\''"#, "4127");

    assert_all_eq(&[r#"``[^ \t\n\r"'`]``"#, r#""[^ \\t\\n\\r\"'`]""#]);
    assert_all_eq(&["```a```", "```\na```"]);
    assert_cdn_eq("```\n``text''```", r#""``text''""#);
    assert_cdn_eq("``` a = ``foo`` ```", r#""a = ``foo``""#);

    assert_hex("(_ h'0123', h'4567')", "5f420123424567ff");
    assert_hex(r#"(_ "foo", "bar")"#, "7f63666f6f63626172ff");
    assert!(cdn_to_vec("(_ )").is_err());
    assert_hex("''_", "5fff");
    assert_hex("\"\"_", "7fff");
    assert_hex("(_ '')", "5f40ff");
    assert_hex(r#"(_ "")"#, "7f60ff");
}

#[test]
fn draft26_base_encoding_and_sequence_examples() {
    assert_all_eq(&["b64`Zm9v`", r#"b64<<"Zm9v">>"#, "b64<<`Zm9v`>>"]);
    assert_cdn_eq("b64`Zm9v`", "'foo'");
    assert_all_eq(&["h'12345678'", "b64'EjRWeA'", "h`12345678`", "b64`EjRWeA`"]);
    assert_all_eq(&[
        "h'48656c6c6f20776f726c64'",
        "h'48 65 6c 6c 6f 20 77 6f 72 6c 64'",
        "h'4 86 56c 6c6f\n        20776 f726c64'",
    ]);
    assert_all_eq(&[
        "h'68656c6c6f20776f726c64'",
        "h'68 65 6c /doubled l!/ 6c 6f # hello\n        20 /space/\n        77 6f 72 6c 64'",
    ]);
    assert_all_eq(&[
        "b64'/base64 not a comment/ but one follows # comment'",
        "h'FDB6AC 7BAE27A2D69CA2699E9EDFDBBADA2779FA25 968C2C'",
    ]);
    assert_cdn_eq("h'/head/ 63 /contents/ 66 6f 6f'", r#"<< "foo" >>"#);

    assert_cdn_eq("<<1>>", "h'01'");
    assert_cdn_eq("<<1, 2>>", "h'0102'");
    assert_cdn_eq(r#"<< "hello", null >>"#, "h'65 68656c6c6f f6'");
    assert_cdn_eq("<<>>", "h''");
    assert_cdn_eq("<<1_1>>", "h'190001'");
    assert_cdn_eq("<<1_0, 2_2>>", "h'1801 1a00000002'");
    assert_cdn_eq(r#"<< "hello"_0, null >>"#, "h'7805 68656c6c6f f6'");
}

#[test]
fn draft26_array_map_tag_and_simple_examples() {
    assert_all_eq(&[
        "[1, 2, 3]",
        "[1, 2, 3,]",
        "[1  2  3]",
        "[1  2  3,]",
        "[1  2, 3]",
        "[1  2, 3,]",
        "[1, 2  3]",
        "[1, 2  3,]",
    ]);
    assert_all_eq(&[
        r#"{1: "n", "x": "a"}"#,
        r#"{1: "n", "x": "a",}"#,
        r#"{1: "n"  "x": "a"}"#,
    ]);
    assert_ne!(bytes("[11]"), bytes("[1 1]"));
    assert_cdn_eq("[1 1]", "[1,1]");
    assert_cdn_eq("[[] []]", "[[],[]]");
    assert!(cdn_to_vec("[[][]]").is_err());

    assert_hex(r#"{1: "to", 1: "from"}"#, "a20162746f016466726f6d");
    assert_hex(
        r#"0("2013-03-21T20:04:00Z")"#,
        "c074323031332d30332d32315432303a30343a30305a",
    );
    assert_hex("1(1363896240)", "c11a514b67b0");
    assert_hex("1_1(1363896240)", "d900011a514b67b0");
    assert_hex("simple(42)", "f82a");
    assert_hex("simple(20)", "f4");
}

#[test]
fn draft26_dt_and_ip_examples() {
    assert_cdn_eq("dt'1969-07-21T02:56:16Z'", "-14159024");
    assert_cdn_eq("dt'1969-07-21T02:56:16.0Z'", "-14159024.0");
    assert_cdn_eq("dt'1969-07-21T02:56:16.5Z'", "-14159023.5");
    assert_cdn_eq("dt`1969-07-21T02:56:16.5Z`", "-14159023.5");
    assert_cdn_eq("dt<<'1969-07-21T02:56:16.5Z'>>", "-14159023.5");
    assert_cdn_eq(r#"dt<<"1969-07-21T02:56:16.5Z">>"#, "-14159023.5");
    assert_cdn_eq("dt<<`1969-07-21T02:56:16.5Z`>>", "-14159023.5");
    assert_cdn_eq("DT'1969-07-21T02:56:16Z'", "1(-14159024)");

    assert_cdn_eq("ip'192.0.2.42'", "h'c000022a'");
    assert_cdn_eq("ip<<'192.0.2.42'>>", "h'c000022a'");
    assert_cdn_eq("IP'192.0.2.42'", "52(h'c000022a')");
    assert_cdn_eq("IP'192.0.2.0/24'", "52([24,h'c00002'])");
    assert_cdn_eq("ip'2001:db8::42'", "h'20010db8000000000000000000000042'");
    assert_cdn_eq(
        "IP'2001:db8::42'",
        "54(h'20010db8000000000000000000000042')",
    );
    assert_cdn_eq("IP'2001:db8::/64'", "54([64,h'20010db8'])");
    assert_cdn_eq("ip'2001:db8::/56'", "[56,h'20010db8']");
    assert_cdn_eq("ip'192.0.2.0/24'", "[24,h'c00002']");
    assert_cdn_eq("52([ip'192.0.2.42',24])", "52([h'c000022a',24])");
    assert_cdn_eq(
        "54([ip'fe80::0202:02ff:ffff:fe03:0303',64,42])",
        "54([h'fe8000000000020202fffffffe030303',64,42])",
    );
}

#[test]
fn draft26_string_concat_and_indefinite_examples() {
    assert_all_eq(&[
        r#""Hello world""#,
        r#"t1<<"Hello ", "world">>"#,
        r#"t1<<"Hello", h'20', "world">>"#,
        "t1<<h'48656c6c6f20776f726c64'>>",
    ]);
    assert_all_eq(&[
        "'Hello world'",
        r#"b1<<"Hello world">>"#,
        "b1<<'Hello ', 'world'>>",
        "b1<<'Hello ', h'776f726c64'>>",
        "b1<<'Hello', h'20', 'world'>>",
        "b1<<h'48656c6c6f20776f726c64', '', b64''>>",
        "b1<<h'4 86 56c 6c6f', h' 20776 f726c64'>>",
    ]);
    assert_all_eq(&[
        "h'48656c6c6f...776f726c64'",
        "b1<<h'48656c6c6f...', ..., h'...776f726c64'>>",
        "b1<<'Hello', ..., 'world'>>",
    ]);

    assert_hex("'Hello world'", "4b48656c6c6f20776f726c64");
    assert_hex("ilbs<<>>", "5fff");
    assert_hex(r#"ilbs<<"Hello world">>"#, "5f4b48656c6c6f20776f726c64ff");
    assert_hex(
        r#"ilbs<<'Hello ', "world">>"#,
        "5f4648656c6c6f2045776f726c64ff",
    );
    assert_hex(
        "ilbs<<'Hello '_0, 'world'>>",
        "5f580648656c6c6f2045776f726c64ff",
    );
    assert!(cdn_to_vec("ilbs<<...>>").is_err());
    assert!(cdn_to_vec("ilts<<...>>").is_err());
}

#[test]
fn draft26_float_examples() {
    assert_hex(
        "[float'fe00', float'fe00'_2, float'47110815']",
        "83f9fe00faffc00000fa47110815",
    );
    assert_hex(
        "[float'fe00', float'fe00'_2, float'47110815', 0x1.22102ap+15]",
        "84f9fe00faffc00000fa47110815fa47110815",
    );
}

#[test]
fn draft26_elision_examples() {
    assert_cdn_eq("[1, 2, ..., 3]", "[1, 2, 888(null), 3]");
    assert_cdn_eq(
        r#"{ "a": 1,
     "b": ...,
     ...: ...
   }"#,
        r#"{ "a": 1,
     "b": 888(null),
     888(null): 888(null)
   }"#,
    );
    assert_cdn_eq(
        r#"{ "contract": t1<<"Herewith I buy", ..., "gned: Alice & Bob">>
     "bytes_in_IRI": b1<<'https://a.example/', ..., '&q=Übergrößenträger'>>
     "signature": h'4711...0815',
   }"#,
        r#"{ "contract": 888(["Herewith I buy", 888(null),
                           "gned: Alice & Bob"]),
     "bytes_in_IRI": 888(['https://a.example/', 888(null),
                          '&q=Übergrößenträger']),
     "signature": 888([h'4711', 888(null), h'0815']),
   }"#,
    );
    assert_cdn_eq(
        r#"{ "signature": h'4711/.../0815',
     # ...: ...
   }"#,
        r#"{ "signature": h'47110815' }"#,
    );
}

#[test]
fn draft26_appendix_a_cdn_examples() {
    assert_all_eq(&[
        "{ / alg / 1: -7 / ECDSA 256 / }",
        "{ 1:   # alg\n             -7 # ECDSA 256\n         }",
    ]);
    assert_cdn_eq(
        r#"98([h'', # empty encoded protected header
             {},  # empty unprotected header
             ...  # rest elided here
            ])"#,
        "98([h'', {}, 888(null)])",
    );
    assert_cdn_eq(
        r#"98([<< {/alg/ 1: -7 /ECDSA 256/} >>, # == h'a10126'
             ...                              # rest elided here
            ])"#,
        "98([h'a10126', 888(null)])",
    );
}

#[cfg(not(feature = "cdn"))]
#[test]
fn draft26_unresolved_app_examples_without_cdn_feature() {
    assert_cdn_eq(
        "cri'https://example.com'",
        r#"999(["cri", ["https://example.com"]])"#,
    );
    assert_cdn_eq(r#"hash<<"data", -44>>"#, r#"999(["hash", ["data", -44]])"#);
}

#[cfg(feature = "cdn")]
#[test]
fn draft26_hash_and_cri_examples() {
    assert_cdn_eq(
        "cri'https://example.com/bottarga/shaved'",
        r#"[-4, ["example", "com"], ["bottarga", "shaved"]]"#,
    );
    assert_cdn_eq(
        "CRI'https://example.com/bottarga/shaved'",
        r#"99([-4, ["example", "com"], ["bottarga", "shaved"]])"#,
    );
    assert_cdn_eq(
        "hash<<'foo'>>",
        "h'2C26B46B68FFC68FF99B453C1D30413413422D706483BFA0F98A5E886266E7AE'",
    );
    assert_cdn_eq(
        "hash'foo'",
        "h'2C26B46B68FFC68FF99B453C1D30413413422D706483BFA0F98A5E886266E7AE'",
    );
    assert_cdn_eq(
        "hash<<'foo', -16>>",
        "h'2C26B46B68FFC68FF99B453C1D30413413422D706483BFA0F98A5E886266E7AE'",
    );
    assert_cdn_eq(
        r#"hash<<'foo', "SHA-256">>"#,
        "h'2C26B46B68FFC68FF99B453C1D30413413422D706483BFA0F98A5E886266E7AE'",
    );
    assert_cdn_eq(
        "hash<<'foo', -44>>",
        concat!(
            "h'F7FBBA6E0636F890E56FBBF3283E524C",
            "6FA3204AE298382D624741D0DC663832",
            "6E282C41BE5E4254D8820772C5518A2C",
            "5A8C0C7F7EDA19594A7EB539453E1ED7'"
        ),
    );
    assert_cdn_eq(
        r#"hash<<'foo', "SHA-512">>"#,
        concat!(
            "h'F7FBBA6E0636F890E56FBBF3283E524C",
            "6FA3204AE298382D624741D0DC663832",
            "6E282C41BE5E4254D8820772C5518A2C",
            "5A8C0C7F7EDA19594A7EB539453E1ED7'"
        ),
    );
}

#[test]
fn parses_sequences_and_deserializes() {
    assert_eq!(
        cdn_sequence_to_vec("1 {\"two\": 2}").unwrap(),
        Vec::from([0x01, 0xa1, 0x63, b't', b'w', b'o', 0x02])
    );
    let value: Value = from_cdn("{1: [2, 3]}").unwrap();
    assert_eq!(value.to_string(), "{1: [2, 3]}");
}

#[test]
fn rejects_invalid_cdn() {
    for input in [
        "",
        "[1[]]",
        "h'0'",
        "'\\u{41}'",
        "dt'2015-01-01T23:59:60Z'",
        "simple(24)",
        "simple(042)",
        "simple(59)_i",
        "1.5_i",
        "1.1_1",
        "float'00'",
        "b64'AA=A'",
        "b64'AA='",
        "b64'AAA=='",
        "b64'===='",
        "b64'SG=V'",
        "ilts<<h'ff'>>",
    ] {
        assert!(cdn_to_vec(input).is_err(), "{input}");
    }
}
