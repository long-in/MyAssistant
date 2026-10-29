# サードパーティの表記

## Silero VAD

`src-tauri/resources/ggml-silero-v5.1.2.bin` は、Silero VAD v5.1.2 を whisper.cpp 用の ggml 形式に変換したモデルです。[ggml-org/whisper-vad](https://huggingface.co/ggml-org/whisper-vad) から取得しています。

- 元のモデル: [snakers4/silero-vad](https://github.com/snakers4/silero-vad)
- ライセンス: MIT License, Copyright (c) 2020-present Silero Team

```text
MIT License

Copyright (c) 2020-present Silero Team

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

## 実行時にダウンロードするモデル

Whisper のモデル(`ggml-large-v3-turbo-q5_0.bin`、`ggml-small.bin`)はリポジトリに含まれず、アプリが初回にダウンロードします。Whisper は OpenAI による MIT License のモデルです。
