# cbor2-cli

在终端中检查、转换和调试 CBOR ([RFC 8949](https://www.rfc-editor.org/rfc/rfc8949))。此 crate 会安装基于 [`cbor2`](https://crates.io/crates/cbor2) 构建的 `cbor` 命令。

[English](README.md) | 简体中文

```bash
cargo install cbor2-cli   # 安装 cbor 二进制文件
```

或者从 ldclabs 的 Homebrew tap 安装：

```bash
brew install ldclabs/tap/cbor2-cli   # 安装 cbor 二进制文件
```

```text
用法: cbor [COMMAND] [INPUT]

命令:
  (无)    将每个 CBOR 项显示为美化排版的诊断表示法 (§8)
  decode  将 CBOR 项转换为美化排版的 JSON，或使用 --diag
          将其转换为美化排版的诊断表示法
  encode  将 JSON 值转换为 CBOR 项，或使用 --diag 将 CDN 文本转换为 CBOR 项
  validate
          验证一个或多个完整的 CBOR 项
```

## 为什么选择 cbor2-cli

| 需求                 | 命令支持                                                                                                              |
| -------------------- | --------------------------------------------------------------------------------------------------------------------- |
| 检查粘贴的 CBOR      | 运行 `cbor <十六进制或-base64>` 来渲染 RFC 8949 诊断表示法。                                                          |
| 保留传输格式细节     | 原生 `cbor` 命令将每个数据项捕获为原始字节，从而使不定长、分段字符串、`undefined` 和简单值保持可见。                  |
| 为 JSON 工具进行解码 | `cbor decode` 将 CBOR 美化输出为 JSON，每个数据项生成一个文档。                                                       |
| 编码测试数据         | `cbor encode` 将 JSON 值转换为 CBOR 字节；`cbor encode --diag` 读取简明诊断表示法 (CDN)。                             |
| 安全复制字节         | `cbor encode --hex` 打印出可复制的小写十六进制文本，方便智能体和文档使用；与 `--diag` 结合可生成 CDN 格式的测试数据。 |
| 处理序列             | 多个 JSON 或 CDN 值会组合成一个 CBOR 序列；CBOR 序列支持逐项解码。                                                    |
| 验证输入             | `cbor validate <十六进制或文件>` 检查一个或多个完整的 CBOR 项，并在成功时打印 `valid`。                               |
| 便于脚本编写         | 数据错误以状态码 1 退出，用法错误以状态码 2 退出。                                                                    |

`INPUT` 可以是文件路径、十六进制字符串（可选带 `0x` 前缀）、base64/base64url 字符串，或者代表标准输入（stdin）的 `-`（默认值）。包含路径分隔符的参数始终会被视为文件路径。所有命令都支持流式处理：多个 JSON 或 CDN 值会组合成一个 CBOR 序列（RFC 8742），而一个 CBOR 序列的每个数据项会输出为对应的文档或行。数据错误以状态码 1 退出，用法错误以状态码 2 退出。

## 智能体友好用法

对于代码智能体（Agent），除非管道需要原始字节，否则推荐使用文本优先的命令：

```bash
cbor validate a1616101
echo '{"a":1}' | cbor encode --hex
printf "{1: h'dead'}" | cbor encode --diag --hex
cbor decode a1616101
cbor decode --diag bf616101ff
```

仅在直接管道传输到另一个二进制命令时使用不带参数的 `cbor encode`。当需要将结果粘贴到测试、提示词、审查评论或其他 `cbor` 调用中时，请使用 `cbor encode --hex`。对于相比 JSON 更容易用 CDN 编写的测试数据，请使用 `cbor encode --diag --hex`。

## 显示：`cbor`

最常用的命令。它将每个数据项渲染为 RFC 8949 第 8 节中定义的、可读性强的文本形式（这也是 CBOR 规范和测试向量的编写格式）。它是完全精确的：每个数据项都会作为其传输字节被捕获，因此不定长的数据项仍保留其 `_` 标记，分段字符串仍显示为 `(_ ...)`，`undefined` 和未分配的简单值按原样显示，字节字符串渲染为 `h'...'`，而大数（bignum）则输出为普通整数，完全符合 RFC 8949 附录 A。非常大的大数负载会退化为显式的 标签/字节 表示法，以保证渲染性能可控。

```bash
$ cbor a201020326                  # 十六进制，直接从规范中复制而来
{
  1: 2,
  3: -7
}

$ cbor 0x8301820203820405          # 0x 前缀的输入也能正常工作
[
  1,
  [
    2,
    3
  ],
  [
    4,
    5
  ]
]

$ cbor oWFhAQ                      # base64url，带或不带填充都行
{
  "a": 1
}

$ cbor message.cbor                # 一个文件
16([
  h'a1010a',
  {
    5: h'89f52f65a1c580933b5261a78c'
  },
  h'5974e1b9...'
])

$ cbor bf61610161629f0203ffff      # 传输细节得以保留
{_
  "a": 1,
  "b": [_
    2,
    3
  ]
}
```

## decode

`cbor decode` 将每个数据项美化输出为 JSON，或者（搭配 `-d`/`--diag`）输出为带缩进的诊断表示法。诊断表示法路径会读取原始数据项字节，因此它保留了不定长度和其他传输细节；JSON 路径则通过 `Value` 进行解码，因此使用的是兼容 JSON 的拼写方式。

```bash
$ cbor decode a1018202036466697665f5
{
  "1": [
    2,
    3
  ]
}
"five"
true

$ cbor decode --diag a101820203
{
  1: [
    2,
    3
  ]
}
```

在 CBOR 特性更丰富的地方，转换为 JSON 时采用“尽力而为”的原则：字节字符串转换为小写十六进制字符串，非字符串类型的 map 键被 JSON 编码为字符串，非有限浮点数（non-finite floats）和 `undefined` 转换为 `null`，超出 64 位范围的整数转换为字符串，并丢弃标签（但保留其内部值）。

## encode

`cbor encode` 读取 JSON 文本（来自文件或标准输入）并将每个值作为 CBOR 项写入。添加 `--diag` 选项可以改为读取简明诊断表示法 (CDN)，添加 `--hex` 选项可以输出便于复制的小写十六进制文本：

```bash
$ echo '{"name": "example", "ok": true}' | cbor encode | cbor
{"name": "example", "ok": true}

$ echo '{"name": "example", "ok": true}' | cbor encode | xxd -p
a2646e616d65676578616d706c65626f6bf5

$ echo '{"name": "example", "ok": true}' | cbor encode --hex
a2646e616d65676578616d706c65626f6bf5

$ printf "{ /kty/ 1: 4, /k/ -1: h'6684523a' }" | cbor encode --diag --hex
a2010420446684523a
```

## validate

`cbor validate` 检查输入是否包含一个或多个完整的 CBOR 项。在成功时它会打印 `valid`，如果数据格式损坏则以状态码 1 退出，如果是用法错误则以状态码 2 退出：

```bash
$ cbor validate a1616101
valid
```

## 许可协议

采用 MIT 许可协议。
