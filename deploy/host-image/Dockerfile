# Disposable Ubuntu 24.04 userland for host-installer proofs.
# Production apps stay on systemd on the VPS; this image only executes
# useradd/visudo/tmpfiles/sysctl/oneshot scripts that Darwin cannot run.
FROM ubuntu:24.04

ENV DEBIAN_FRONTEND=noninteractive \
    LANG=C.UTF-8

RUN apt-get update \
    && apt-get install -y --no-install-recommends \
        adduser \
        ca-certificates \
        curl \
        openssl \
        openssh-client \
        passwd \
        procps \
        python3 \
        e2fsprogs \
        iproute2 \
        nftables \
        slirp4netns \
        fuse-overlayfs \
        uidmap \
        sudo \
        systemd \
        systemd-sysv \
        util-linux \
    && rm -rf /var/lib/apt/lists/* \
    && arch=$(dpkg --print-architecture) \
    && case "$arch" in \
         amd64) goarch=amd64 ;; \
         arm64) goarch=arm64 ;; \
         *) printf 'unsupported dpkg architecture: %s\n' "$arch" >&2; exit 1 ;; \
       esac \
    && curl -fsSL "https://github.com/caddyserver/caddy/releases/download/v2.10.2/caddy_2.10.2_linux_${goarch}.tar.gz" \
       | tar -xz -C /usr/local/bin caddy \
    && chmod 0755 /usr/local/bin/caddy

CMD ["/bin/bash", "/src/deploy/host-image/prove-installers.sh"]
