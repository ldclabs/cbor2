# cbor2-cli

在终端中检查、转换和调试 CBOR
（[RFC 8949](https://www.rfc-editor.org/rfc/rfc8949)）。本 crate 安装 `cbor`
命令，构建于 [`cbor2`](https://crates.io/crates/cbor2) 之上。

[English](README.md) | 简体中文

```bash
cargo install cbor2-cli   # 安装 `cbor` 二进制文件
```

或从 ldclabs 的 Homebrew tap 安装：

```bash
brew install ldclabs/tap/cbor2-cli   # 安装 `cbor` 二进制文件
```

```text
Usage: cbor [COMMAND] [INPUT]

Commands:
  (none)  将每个 CBOR 项显示为一行诊断记法（§8）
  decode  将 CBOR 项转换为美化的 JSON，或用 --diag
          转换为美化的诊断记法
  encode  将 JSON 值转换为 CBOR 项
  validate
          校验一个或多个完整 CBOR 项
```

## 为什么选择 cbor2-cli

| 需求             | 命令支持                                                                                                   |
| ---------------- | ---------------------------------------------------------------------------------------------------------- |
| 检查粘贴的 CBOR  | 运行 `cbor <hex-or-base64>` 渲染 RFC 8949 诊断记法。                                                       |
| 保留传输细节     | 裸 `cbor` 将每一项捕获为原始字节，因此不定长、分段字符串、`undefined` 和简单值（simple value）都保持可见。 |
| 为 JSON 工具解码 | `cbor decode` 将 CBOR 美化输出为 JSON，每项一个文档。                                                      |
| 编码测试夹具     | `cbor encode` 将 JSON 值转换为 CBOR 字节；`cbor encode --hex` 为 agent 和文档输出可复制的小写十六进制。    |
| 处理序列         | 多个 JSON 值会成为一个 CBOR 序列；CBOR 序列逐项解码。                                                      |
| 校验输入         | `cbor validate <hex-or-file>` 校验一个或多个完整 CBOR 项，成功时输出 `valid`。                             |
| 可靠地编写脚本   | 数据错误以状态码 1 退出，用法错误以状态码 2 退出。                                                         |

`INPUT` 是文件路径、十六进制字符串（可带 `0x` 前缀）、base64/base64url
字符串，或表示 stdin 的 `-`；默认是 stdin。包含路径分隔符的参数总是被视为
文件路径。一切都是流式的：多个 JSON 值会成为一个 CBOR 序列（RFC 8742），
而一个 CBOR 序列会成为每项一个输出文档或一行。数据错误以状态码 1 退出，
用法错误以状态码 2 退出。

## 面向 agent 的用法

代码 agent 应优先使用文本优先的命令，除非需要把原始字节直接管道传给另一个二进制命令：

```bash
cbor validate a1616101
echo '{"a":1}' | cbor encode --hex
cbor decode a1616101
cbor decode --diag bf616101ff
```

只有在直接管道传输时才使用裸 `cbor encode`。当结果需要粘贴到测试、prompt、review
评论或另一个 `cbor` 调用里时，使用 `cbor encode --hex`。

## 显示：`cbor`

日常命令。它将每一项渲染为 RFC 8949 §8 的人类可读文本形式 —— 也就是 CBOR
规范和测试向量所采用的写法 —— 并且是精确的：每一项都按其传输字节捕获，因此
不定长项保留其 `_` 标记，分段字符串显示为 `(_ ...)`，`undefined` 和未分配
的简单值如实显示，字节串渲染为 `h'...'`，bignum 打印为普通整数，与
RFC 8949 附录 A 完全一致。

```bash
$ cbor a201020326                  # 十六进制，直接从规范粘贴
{1: 2, 3: -7}

$ cbor 0x8301820203820405          # 带 0x 前缀也可以
[1, [2, 3], [4, 5]]

$ cbor oWFhAQ                      # base64url，是否带填充均可
{"a": 1}

$ cbor message.cbor                # 一个文件
16([h'a1010a', {5: h'89f52f65a1c580933b5261a78c'}, h'5974e1b9...'])

$ cbor bf61610161629f0203ffff      # 传输细节得以保留
{_ "a": 1, "b": [_ 2, 3]}
```

## decode

`cbor decode` 将每一项解码进数据模型并美化输出为 JSON，或用 `-d`/`--diag`
输出为缩进的诊断记法。与裸 `cbor` 不同，它会重新拼写该项（不定长和非首选
编码不会被保留）。

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

在 CBOR 比 JSON 更丰富之处，转换为 JSON 是尽力而为的：字节串变为小写十六
进制字符串，非字符串 map 键被 JSON 编码进字符串，非有限浮点数和 `undefined`
变为 `null`，超出 64 位范围的整数变为字符串，标签则被丢弃（保留内层值）。

## encode

`cbor encode` 读取 JSON 文本（来自文件或 stdin），并将每个值写为一个 CBOR
项。加上 `--hex` 可输出可复制的小写十六进制文本：

```bash
$ echo '{"name": "example", "ok": true}' | cbor encode | cbor
{"name": "example", "ok": true}

$ echo '{"name": "example", "ok": true}' | cbor encode | xxd -p
a2646e616d65676578616d706c65626f6bf5

$ echo '{"name": "example", "ok": true}' | cbor encode --hex
a2646e616d65676578616d706c65626f6bf5
```

## validate

`cbor validate` 检查输入是否包含一个或多个完整 CBOR 项。成功时输出 `valid`，
格式错误以状态码 1 退出，用法错误以状态码 2 退出：

```bash
$ cbor validate a1616101
valid
```

## 许可

以 MIT 许可发布。
