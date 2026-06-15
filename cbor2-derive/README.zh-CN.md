# cbor2-derive

为 [`cbor2`](https://crates.io/crates/cbor2) 提供面向协议形态 CBOR 的派生
支持。

[English](README.md) | 简体中文

大多数用户不应直接依赖本 crate，而应在 `cbor2` 上启用 `derive` 特性：

```toml
[dependencies]
cbor2 = { version = "1", features = ["derive"] }
serde_bytes = "0.11" # 仅在如下例的二进制字段中需要
```

## 为什么选择 cbor2-derive

`serde` 派生对常见数据模型非常出色，但某些 CBOR 协议需要 serde 属性无法直接
表达的传输细节：整数 map 键、字段顺序数组、语义标签和 COSE 风格的紧凑结构。
`#[derive(cbor2::Cbor)]` 为这类形态生成 serde 实现。

| 需求         | 内置能力                                                                                 |
| ------------ | ---------------------------------------------------------------------------------------- |
| 整数 map 键  | `#[cbor(key = 1)]` 写入真正的 CBOR 整数键，而非文本键 `"1"`。                            |
| 字段顺序数组 | `#[cbor(array)]` 将具名结构体编码为紧凑的 CBOR 数组，同时保留 Rust 字段名。              |
| 语义标签     | `#[cbor(tag = 18)]` 将编码后的项包裹进一个 CBOR 标签，并在解码时接受带或不带标签的输入。 |
| COSE 易用性  | 紧凑的 RFC 9052 结构可直接在 Rust 结构体和元组结构体上声明。                             |
| JSON 兼容性  | 字段名和类型名保持不变，因此 `serde_json` 仍使用自然名称且没有 CBOR 标签。               |
| 运行时元数据 | 生成的 `cbor2::Cbor` 实现暴露 `T::KEYS`、`T::TAG`、`T::ARRAY` 和 `value.keys()`。        |
| Serde 属性   | 诸如 `default`、`skip`、`alias` 和 `with = "serde_bytes"` 等字段级属性仍然有效。         |
| 扁平扩展字段 | map 形态结构体上的 `#[serde(flatten)]` 可承载扩展字段，包括 COSE 风格的整数/text 标签键。 |

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

对于传输形态为数组、但带具名 Rust 字段的 COSE 消息：

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

对于 CWT 这类带扩展 claim 的 map 形态协议：

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

已声明字段仍使用对应的 CBOR 整数键，扁平 map 中的扩展字段保留普通文本键。
如果扁平 map 的键类型会序列化为整数或字符串，例如 COSE `Label` / `CoseMap`，
CBOR 往返时也会保留这些整数键。`#[serde(flatten)]` 支持具名的 map 结构体；
不要与 `#[cbor(array)]` 混用。

该宏会生成 `serde::Serialize`、`serde::Deserialize` 和 `cbor2::Cbor`。请勿
在同一类型上同时派生 serde 的 `Serialize` 或 `Deserialize`；那些实现会冲突。

完整的 COSE 示例参见主
[`cbor2` README](https://github.com/ldclabs/cbor2#integer-map-keys-and-tags-cose-with-derivecbor)。

## 许可

以 MIT 许可发布。
