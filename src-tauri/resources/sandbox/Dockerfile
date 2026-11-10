# MyAssistant エージェント用サンドボックスの標準イメージ。
# このファイルはエージェントのワークスペース(workspaces/<ID>/sandbox/Dockerfile)へコピーされたもので、自由に編集できます。
# 保存すると、次にコマンドを実行するときに自動で再ビルドされます(ビルドの文脈はこのフォルダだけです)。
#
# 変更するときの前提(崩れると実行前の診断でエラーになります):
#   - 非 root のユーザー `agent` があり、実行はそのユーザーで行う
#   - `/workspace`(ナレッジを置く場所)と `/home/agent`(専用ボリューム)が存在する
#   - `sh` と `setsid`(util-linux)が使える
# 認証情報はビルドには渡されません。実行時の環境変数は sandbox.env に書きます。
FROM debian:bookworm-slim

RUN apt-get update \
    && apt-get install -y --no-install-recommends \
        ca-certificates curl unzip git jq less procps util-linux \
        python3 python3-pip nodejs npm \
    && rm -rf /var/lib/apt/lists/*

# AWS CLI v2(uname -m は aarch64 / x86_64 で、配布ファイル名と一致する)
RUN curl -fsSL "https://awscli.amazonaws.com/awscli-exe-linux-$(uname -m).zip" -o /tmp/awscliv2.zip \
    && unzip -q /tmp/awscliv2.zip -d /tmp \
    && /tmp/aws/install \
    && rm -rf /tmp/aws /tmp/awscliv2.zip

RUN useradd --create-home --home-dir /home/agent --uid 10001 --shell /bin/sh agent \
    && mkdir -p /workspace \
    && chown agent:agent /workspace

WORKDIR /workspace
USER agent
CMD ["sleep", "infinity"]
