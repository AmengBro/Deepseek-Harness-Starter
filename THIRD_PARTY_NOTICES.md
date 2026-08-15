# 第三方组件声明（Third-Party Notices）

本项目（DeepseekHarnessStarter）以 GPL-3.0 发布。以下第三方组件各自采用其声明的许可证，再分发本项目（或其构建产物）时，应保留下列版权与许可声明。

## 核心运行时：@deepseek-ai/dsh

- 来源：[deepseek-ai/deepseek-harness](https://github.com/deepseek-ai/deepseek-harness)
- 许可证：MIT

```text
MIT License

Copyright (c) 2026 DeepSeek

Permission is hereby granted, free of charge, to any person obtaining a copy
of this software and associated documentation files (the "Software"), to deal
in the Software without restriction, including without limitation the rights
to use, copy, modify, merge, publish, distribute, sublicense, and/or sell
copies of the Software, and to permit persons to whom the Software is
furnished to do so, subject to the following conditions:

The above copyright notice and this permission notice shall be included in all
copies or substantial portions of the Software.

THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND, EXPRESS OR
IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY,
FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE
AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER
LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM,
OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE
SOFTWARE.
```

## Tauri 系列

- 组件：`tauri`、`tauri-build`（Rust）、`@tauri-apps/api`、`@tauri-apps/cli`（npm）
- 来源：[tauri-apps/tauri](https://github.com/tauri-apps/tauri)
- 许可证：MIT OR Apache-2.0
- 版权：© 2019-2025 Tauri Contributors

MIT 与 Apache-2.0 的完整许可文本见其仓库根目录下的 `LICENSE_MIT` 与 `LICENSE_APACHE-2.0`。

## 其他 Rust 依赖

| 组件 | 许可证 |
|---|---|
| serde / serde_json | MIT OR Apache-2.0 |
| toml | MIT OR Apache-2.0 |
| tokio | MIT |
| chrono | MIT OR Apache-2.0 |
| reqwest | MIT OR Apache-2.0 |
| anyhow | MIT OR Apache-2.0 |

## 其他前端依赖

| 组件 | 许可证 |
|---|---|
| typescript | Apache-2.0 |
| vite | MIT |

各组件完整的许可证文本请参见对应官方仓库，或 `node_modules/<package>/LICENSE`、本地 Cargo registry 中对应 crate 的 LICENSE 文件。
