# cbor2-derive

为 [`cbor2`](https://crates.io/crates/cbor2) 提供符合协议格式的 CBOR 派生（derive）支持。

[English](README.md) | 简体中文

大多数用户不需要直接依赖此 crate。请改为启用 `cbor2` 的 `derive` 特性：

```toml
[dependencies]
cbor2 = { version = "1", features = ["derive"] }
serde_bytes = "0.11" # 仅针对如下方示例中的二进制字段才需要
```

## 为什么选择 cbor2-derive

`serde` 的派生宏对于常规数据模型表现优异，但某些 CBOR 协议需要一些 `serde` 属性无法直接表达的传输格式细节：整数 map 键、字段顺序数组、语义标签和 COSE 风格的紧凑结构。
`#[derive(cbor2::Cbor)]` 为这些格式生成对应的 `serde` 实现。

| 需求         | 内置支持                                                                                                     |
| ------------ | ------------------------------------------------------------------------------------------------------------ |
| 整数 map 键  | `#[cbor(key = 1)]` 会写入真正的 CBOR 整数键，而不是文本键 `"1"`。                                            |
| 字段顺序数组 | `#[cbor(array)]` 会将命名的结构体编码为紧凑的 CBOR 数组，同时在 Rust 中保留结构体的字段名。                  |
| 语义标签     | `#[cbor(tag = 18)]` 会将编码后的数据项包装在 CBOR 标签中，并在反序列化（解码）时兼容带标签或不带标签的输入。 |
| COSE 易用性  | 可以在 Rust 结构体和元组结构体上直接声明紧凑的 RFC 9052 结构。                                               |
| JSON 兼容性  | 字段名和类型名保持不变，因此 `serde_json` 仍然可以使用自然名称，且不含 CBOR 标签。                           |
| 运行时元数据 | 生成的 `cbor2::Cbor` 实现公开了 `T::KEYS`、`T::TAG`、`T::ARRAY` 和 `value.keys()`。                          |
| Serde 属性   | 字段级属性（如 `default`、`skip`、`alias` 和 `with = "serde_bytes"`）可以继续正常工作。                      |
| 扁平化 map   | map 格式的结构体上的 `#[serde(flatten)]` 可以承载扩展字段，包括 COSE 风格的整数/文本标签。                   |

## 示例

```rust
use cbor2::Cbor;

#[derive(Debug, PartialEq, Cbor)]
#[cbor(tag = 18)]
struct CoseHeader {
    #[cbor(key = 1)]
    alg: i8,
    #[cbor(key = 4)]
    #[serde(with = "serde_bytes")]
    kid: Vec<u8>,
}

assert_eq!(CoseHeader::KEYS, &[("alg", 1), ("kid", 4)]);
assert_eq!(CoseHeader::TAG, Some(18));
```

对于带有 Rust 命名字段的 COSE 数组格式消息：

```rust
use cbor2::Cbor;

#[derive(Debug, PartialEq, Cbor)]
#[cbor(tag = 18, array)]
struct Sign1 {
    #[serde(with = "serde_bytes")]
    protected: Vec<u8>,
    unprotected: u8,
    #[serde(with = "serde_bytes")]
    payload: Vec<u8>,
    #[serde(with = "serde_bytes")]
    signature: Vec<u8>,
}

assert!(Sign1::ARRAY);
```

对于带有扩展声明的 map 格式协议，例如 CWT：

```rust
use std::collections::BTreeMap;

use cbor2::{Cbor, Value};

#[derive(Debug, PartialEq, Cbor)]
#[cbor(tag = 61)]
struct Claims {
    #[cbor(key = 1)]
    #[serde(rename = "iss")]
    issuer: String,
    #[serde(flatten, default)]
    extra: BTreeMap<String, Value>,
}
```

声明的字段仍使用其 CBOR 整数键，而扁平化 map 会保留普通的文本键。对于键类型可以序列化为整数或字符串的扁平化 map（例如 COSE `Label` / `CoseMap`），在 CBOR 往返编解码中仍能保留这些整数键。命名 map 结构体支持使用 `#[serde(flatten)]`；请勿将其与 `#[cbor(array)]` 混用。

该宏会自动生成 `serde::Serialize`、`serde::Deserialize` 以及 `cbor2::Cbor` 的实现。请勿在同一类型上同时派生 serde 的 `Serialize` 或 `Deserialize`，否则这些实现会发生冲突。

完整的 COSE 示例请参阅 [`cbor2` 主 README](https://github.com/ldclabs/cbor2#integer-map-keys-and-tags-cose-with-derivecbor)。

## 许可协议

采用 MIT 许可协议。
