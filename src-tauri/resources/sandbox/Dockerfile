# MyAssistant サンドボックスの標準イメージ(自由に編集できます)。
# 変更したら、Sandbox 設定のチェックリストで、イメージのビルドとコンテナの起動をやり直してください。
# 必要なもの: 非 root のユーザー `agent`、`/knowledge`、`sh`、`setsid`(util-linux)、`claude`。
# Claude Code は Node.js 22 以上(標準は 26 系)が必要です。
FROM node:26-bookworm-slim

RUN apt-get update \
    && apt-get install -y --no-install-recommends \
        ca-certificates curl unzip git jq less procps util-linux ripgrep \
        python3 python3-pip \
    && rm -rf /var/lib/apt/lists/*

# AWS CLI v2
RUN curl -fsSL "https://awscli.amazonaws.com/awscli-exe-linux-$(uname -m).zip" -o /tmp/awscliv2.zip \
    && unzip -q /tmp/awscliv2.zip -d /tmp \
    && /tmp/aws/install \
    && rm -rf /tmp/aws /tmp/awscliv2.zip

# Claude Code(既定は最新版。版を書けば固定できる)。ファイルシステムは読み取り専用なので自動更新は止める
# 最新版への更新は、イメージの再ビルドとコンテナの作り直しで行う
ARG CLAUDE_CODE_VERSION=latest
# アプリがビルドのたびに違う値を渡し、この層だけキャッシュを使わずに入れ直す
ARG CLAUDE_CODE_REFRESH=
RUN echo "refresh=${CLAUDE_CODE_REFRESH}" > /dev/null \
    && npm install -g "@anthropic-ai/claude-code@${CLAUDE_CODE_VERSION}" \
    && npm cache clean --force
ENV DISABLE_AUTOUPDATER=1

RUN useradd --create-home --home-dir /home/agent --uid 10001 --shell /bin/sh agent \
    && mkdir -p /knowledge \
    && chown agent:agent /knowledge

WORKDIR /knowledge
USER agent
CMD ["sleep", "infinity"]
