# cbor2：适用于 Rust 的全功能 CBOR

Rust 中的大多数 CBOR 库都只是序列化器 —— 它们负责将结构体转换为字节，以及反向转换，到此为止。但是，[CBOR (RFC 8949)](https://www.rfc-editor.org/rfc/rfc8949) 的功能面比 JSON 丰富得多：语义标签、大数（bignum）、确定性编码、诊断表示法以及不定长流。一旦您需要使用这些特性 —— 比如 COSE、签名负载、规范哈希 —— 通常就必须在序列化器的基础上自己动手实现。

**cbor2** 尝试将这些完整的功能作为一个原生支持 serde 的工具包提供，同时在速度上保持竞争力，并能平滑缩小到无分配器的 `no_std` 环境。

首先交代一下渊源，因为致敬前人很重要：`cbor2` 承袭自 Andrew Gallant 最初创建的 `cbor` crate。0.5 版本是一个基于 serde 和 RFC 8949 的全新重写版，而 1.0 版本使其趋于稳定。在这个领域还有其他优秀的 crate —— `ciborium` 和 `minicbor` 非常坚实，`cbor4ii` 速度很快 —— 而大多数人仍习惯使用的 `serde_cbor` 自 2021 年起就已停止维护。`cbor2` 并非试图去取代其中任何一个，它的目标在于填补“我需要完整的协议支持，而不仅仅是一个序列化器”这一细分需求。

## 同一结构体，两种传输格式

这是我最满意的部分。COSE（RFC 9052 —— WebAuthn、CWT 等背后的签名/加密格式）使用**整数**而非字符串来作为 map 的键，并在消息外层包裹标签。serde 的数据模型无法直接表达这两种特性。通常的解决办法是定义第二套类型，或者进行手动编码。

`cbor2` 允许用同一个结构体同时完成这两件事：

```rust
use cbor2::Cbor;

#[derive(Cbor)]
#[cbor(tag = 98)]
struct CoseSign {
    #[cbor(key = 1)]
    kty: u8,
    #[cbor(key = 3)]
    alg: i8,
    comment: String, // 无 key 属性 → 在传输时保持为文本键
}
```

使用 `cbor2` 进行编码，您将获得 COSE 所期望的紧凑、整数键、带标签 98 的格式。而将*同一个值*传给 `serde_json::to_string`，您将得到普通的 `{"kty": ..., "alg": ..., "comment": ...}` —— 字段名完好无损，且不带标签。在日志和调试端点中使用 JSON，在传输时使用 COSE，只需这一个类型。

在底层，`#[derive(Cbor)]` 通过隐藏的 `#[serde(remote)]` 影子类型和容器名称标记来自动生成 serde 实现，因此真实类型的字段和类型名称绝不会被修改。无需注册表，也无需链接时技巧 —— 在 `wasm32-unknown-unknown` 目标上也一样可以正常工作。

## 当精确字节至关重要时

对于签名和内容寻址来说，生成*哪些*字节正是关键所在。`cbor2` 提供了确定性编码（RFC 8949 §4.2.1 —— 排序的 map 键、最短形式的整数/浮点数、归一化的大数和 NaN）：

```rust
let canonical = cbor2::to_canonical_vec(&value)?;
```

至于另一个方向 —— 验证非您自己生成的字节上的签名 —— 则可以使用 `RawValue`，它将单个数据项捕获为经过验证但未解码的原始字节：

```rust
#[derive(Serialize, Deserialize)]
struct Signed {
    #[serde(with = "serde_bytes")]
    signature: Vec<u8>,
    payload: cbor2::RawValue, // 逐字节捕获
}
// 先在 signed.payload.as_bytes() 上验证 signature，然后再对其进行解码
```

在信任内容之前，您可以通过精确的传输字节（甚至是采用非首选序列化形式的字节）来进行验证。如果通过类型化值进行往返编解码，会默认进行重新编码，从而破坏签名。

此外，还提供了带有 `cbor!` 宏的动态 `Value`、RFC 8949 第 8 节诊断表示法（以及可打印该表示法的 `cbor` 命令行工具），以及可用于检查输入是否恰好为一个格式完好数据项的 `validate` 函数。

## 一路向下延伸至无分配环境 (no_alloc)

`cbor2` 支持 `no_std`。在启用 `alloc` 的情况下，您可以获得完整的基于堆内存分配的 API；在未启用它的情况下，您仍然可以获得序列化、验证和精确大小计算支持 —— 这些操作均不分配内存：

```rust
let mut buf = [0u8; 256];
let item = cbor2::to_slice(&value, &mut buf)?; // 编码到您的缓冲区中
let size = cbor2::serialized_size(&value)?;    // 精确长度，无需输出缓冲区
cbor2::validate(&item[..])?;                   // 格式是否完好？（包括 UTF-8 验证）
```

这里有一个需要坦白说明的限制：**没有任何一个基于 serde 的 CBOR crate 可以在没有堆内存的情况下进行反序列化** —— 因为反序列化器需要一个暂存缓冲区。`cbor2` 也不例外。只有并非基于 serde 的 `minicbor` 能够在没有分配器的情况下解码类型化值。作为替代，`cbor2` 提供的是其底层的 `core::Decoder` 拉取式（pull）API，如果您愿意手动编写解码逻辑，便可以实现零分配读取 CBOR：

```rust
use cbor2::core::{Decoder, Header};

let mut dec = Decoder::from(&bytes[..]);
let Header::Array(Some(n)) = dec.pull()? else { /* ... */ };
// 拉取标头；将字符串/字节实体读取到您自备的 &mut [u8] 中
```

因此，关于无分配（no-alloc）的使用场景是：通过高级 API 实现零分配编码、验证和大小计算；通过 `core::Decoder` 进行手动解码。目前无法支持基于 serde 类型的反序列化解码。我希望您在项目初期就了解这一点，而不是在项目进行到一半时才发现。

## 速度快吗？

快，但我不会声称它是最快的，因为事实并非如此，而且您也一定会去验证。我构建了一个[独立的基准测试工作区](https://github.com/ldclabs/cbor2/tree/main/cbor2-bench#results)，在尽可能使用完全相同的数据负载的情况下，将 `cbor2` 与 `ciborium`、`serde_cbor`、`cbor4ii` 和 `minicbor` 在全部三种部署模式（`std`、`no_std + alloc`、`no_std + no_alloc`）下进行了对比，因此整数数组和字节字符串行是严格同等对比。

坦白地说：`cbor2` 的编码和解码性能位居前列，并且全面超越了 `ciborium`，但 `serde_cbor` 和 `cbor4ii` 在某些编码场景中略微领先，而 `minicbor` 凭借更紧凑的传输形式在结构化解码中处于领先地位。`cbor2` 表现出明显优势的地方在于 `no_std + no_alloc` 环境 —— 它是 serde 类 crate 中固定缓冲区编码速度最快的，并且提供了其他库均未附带的 `validate` / `serialized_size` 原语。（构建这套测试组件还发现了一些有趣的细节，比如 `cbor4ii` 的解码器会拒绝将窄化后的 `f32` 浮点数放入 `f64` 字段中 —— 因此有些 crate 只能解码自身生成的字节。）

完整的功能矩阵、各个场景下的表格以及详细的测试方法，均已收录在[基准测试 README](https://github.com/ldclabs/cbor2/tree/main/cbor2-bench#results) 中。

## 它不具备哪些功能

- 不支持在没有 `alloc` 的情况下进行类型化解码（如上所述，这是目前整个 serde 生态系统的普遍现状）。
- 它还是个全新的项目，所以用户群规模还很小，肯定还有一些不完善的地方。
- 如果您追求极致速度的结构化解码，且可以使用非 serde 的派生宏，那么 `minicbor` 会是更好的选择 —— 这完全没问题。

## 尝试使用

```toml
[dependencies]
cbor2 = "1"
```

- crates.io: <https://crates.io/crates/cbor2>
- docs: <https://docs.rs/cbor2>
- repo: <https://github.com/ldclabs/cbor2>

非常欢迎您的错误报告、设计批评以及“为什么不直接使用 X”之类的疑问 —— 这正是促使我将其分享出来的原因。
