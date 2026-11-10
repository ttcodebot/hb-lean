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
RUN /usr/local/bin/python3 -m venv /opt/tt \
    && /opt/tt/bin/pip install --no-cache-dir -r /opt/tt-requirements.txt \
    && /usr/local/bin/python3 -m venv /opt/precheck \
    && /opt/precheck/bin/pip install --no-cache-dir -r /opt/precheck-requirements.txt \
    && /opt/tt/bin/yowasp-yosys -V && /opt/precheck/bin/yowasp-yosys -V \
    && chmod -R a+rX /opt/cache
COPY pdk /pdk
