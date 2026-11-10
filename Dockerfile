# Lean Tiny Tapeout harden + precheck image (experiment).
# Tools: the subset of the LibreLane image's Nix closure that the flow runs.
FROM python:3.11-slim-bookworm
RUN apt-get update && apt-get install -y --no-install-recommends \
      git jq ca-certificates librsvg2-bin pngquant fontconfig fonts-dejavu-core \
    && rm -rf /var/lib/apt/lists/* \
    && git config --system --add safe.directory '*'
COPY nix /nix
ARG NIX_PATH_DIRS
ARG NIX_PYTHONPATH_ARG
ARG PRECHECK_TOOLS=""
ARG PDK
ENV XDG_CACHE_HOME=/opt/cache \
    SSL_CERT_FILE=/etc/ssl/certs/ca-certificates.crt \
    LANG=C.UTF-8 LC_ALL=C.UTF-8 \
    NIX_PYTHONPATH=${NIX_PYTHONPATH_ARG} \
    PDK_ROOT=/pdk PDK=${PDK} \
    PRECHECK_TOOLS=${PRECHECK_TOOLS} \
    TOOLS_PATH=${NIX_PATH_DIRS} \
    PATH=${NIX_PATH_DIRS}:/usr/local/sbin:/usr/local/bin:/usr/sbin:/usr/bin:/sbin:/bin:/opt/tt/bin
COPY tt-requirements.txt precheck-requirements.txt /opt/
# yowasp-yosys (WASM Yosys, ~350 MB with its compile cache) is replaced by the image's native Yosys.
RUN grep -viE '^(yowasp|wasmtime)' /opt/tt-requirements.txt > /tmp/tt.txt \
    && grep -viE '^(yowasp|wasmtime)' /opt/precheck-requirements.txt > /tmp/pc.txt \
    && /usr/local/bin/python3 -m venv /opt/tt \
    && /opt/tt/bin/pip install --no-cache-dir --no-deps -r /tmp/tt.txt \
    && /usr/local/bin/python3 -m venv /opt/precheck \
    && /opt/precheck/bin/pip install --no-cache-dir --no-deps -r /tmp/pc.txt \
    && printf '#!/bin/sh\nexec yosys "$@"\n' > /usr/local/bin/yowasp-yosys && chmod +x /usr/local/bin/yowasp-yosys \
    && rm /tmp/tt.txt /tmp/pc.txt
COPY pdk /pdk
