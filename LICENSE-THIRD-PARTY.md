# Third Party Licenses

本リポジトリには以下のソフトウェアからの移植・参考を含む。ライセンスは MIT で帰属表示する。

## moocs-collect (MIT)

- Source: https://github.com/yu7400ki/moocs-collect
- License: MIT License, Copyright (c) 2024 Yuki Natori

The following modules are ports/adaptations of code from moocs-collect:

- `crates/imoocs-core/src/auth/moocs.rs` — based on `src/repository/auth.rs:80-99`
- `crates/imoocs-core/src/auth/google.rs` — based on `src/repository/auth.rs:101-185`
- `crates/imoocs-core/src/scrape/courses.rs` — based on `src/repository/course.rs:48-64`
- `crates/imoocs-core/src/scrape/pages.rs` — based on `src/repository/page.rs:53-100`
- `crates/imoocs-core/src/api/slides.rs` — based on `src/repository/slide.rs:56-113`
- `crates/imoocs-core/src/util/html.rs` — based on `src/utils.rs:4-22`

### moocs-collect License Text (reproduced)

```
MIT License

Copyright (c) 2024 Yuki Natori

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
