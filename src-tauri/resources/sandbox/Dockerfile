# MyAssistant サンドボックスの標準イメージ。
# このファイルは共通の Dockerfile(<アプリデータ>/sandbox/Dockerfile)へコピーされたもので、自由に編集できます。
# エージェント専用の Dockerfile を登録すると、そのエージェントは共通ではなく専用を使います(登録時に共通の内容がコピーされます)。
# 保存すると、次の実行時に自動で再ビルドされます(ビルドの文脈はこのフォルダだけです)。
#
# 変更するときの前提(崩れるとコンテナを作った直後の診断でエラーになります):
#   - 非 root のユーザー `agent` があり、実行はそのユーザーで行う
#   - `/workspace`(担当システムのフォルダ)・`/knowledge`(ナレッジ)・`/home/agent`(専用ボリューム)を置ける
#   - `sh` と `setsid`(util-linux)、`claude`(Claude Code)が使える
# 認証情報はビルドには渡されません。実行時の環境変数は sandbox.env に書きます。
#
# Claude Code は Node.js 22 以上が必要です。
FROM node:22-bookworm-slim

RUN apt-get update \
    && apt-get install -y --no-install-recommends \
        ca-certificates curl unzip git jq less procps util-linux ripgrep \
        python3 python3-pip \
    && rm -rf /var/lib/apt/lists/*

# AWS CLI v2(uname -m は aarch64 / x86_64 で、配布ファイル名と一致する)
RUN curl -fsSL "https://awscli.amazonaws.com/awscli-exe-linux-$(uname -m).zip" -o /tmp/awscliv2.zip \
    && unzip -q /tmp/awscliv2.zip -d /tmp \
    && /tmp/aws/install \
    && rm -rf /tmp/aws /tmp/awscliv2.zip

# Claude Code(版は固定。更新はこの行の版を書き換える)
ARG CLAUDE_CODE_VERSION=2.1.311
RUN npm install -g "@anthropic-ai/claude-code@${CLAUDE_CODE_VERSION}" \
    && npm cache clean --force
# コンテナのファイルシステムは読み取り専用なので、自動更新は止める
ENV DISABLE_AUTOUPDATER=1

RUN useradd --create-home --home-dir /home/agent --uid 10001 --shell /bin/sh agent \
    && mkdir -p /workspace /knowledge \
    && chown agent:agent /workspace /knowledge

WORKDIR /workspace
USER agent
CMD ["sleep", "infinity"]
