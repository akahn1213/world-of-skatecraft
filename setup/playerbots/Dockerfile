# syntax=docker/dockerfile:1
# Build and run against the same base image: never copy host-linked binaries in.
ARG BASE_IMAGE
FROM ${BASE_IMAGE} AS build
USER root
ARG BUILD_JOBS=1
RUN apt-get update && apt-get install -y --no-install-recommends \
    build-essential cmake ninja-build git ccache rsync time libboost-dev libmysqlclient-dev \
    libssl-dev zlib1g-dev && rm -rf /var/lib/apt/lists/*
COPY build_cached.sh /build/build_cached.sh
COPY source /build/source
ARG BUILD_CACHE_ID=playerbots-v1
ARG DEBUG_PROFILE=lines
RUN --mount=type=cache,id=${BUILD_CACHE_ID},target=/build/cache,sharing=locked \
    sh /build/build_cached.sh
# Commit metadata must not invalidate compilation.
COPY provenance.json /package/opt/vmangos/share/playerbots/provenance.json

FROM ${BASE_IMAGE} AS runtime
USER root
# gdb plus unstripped binaries permit a backtrace without host core-dump setup.
RUN apt-get update && apt-get install -y --no-install-recommends gdb && rm -rf /var/lib/apt/lists/*
COPY --from=build /package/opt/vmangos /opt/vmangos
RUN ldd /opt/vmangos/bin/mangosd > /tmp/playerbots-libraries && \
    ldd /opt/vmangos/bin/realmd >> /tmp/playerbots-libraries && \
    ! grep -q 'not found' /tmp/playerbots-libraries && \
    /opt/vmangos/bin/mangosd --version && /opt/vmangos/bin/realmd --version && \
    cd /opt/vmangos && sha256sum -c share/playerbots/binaries.sha256 && \
    gdb --batch -ex "info line RandomPlayerbotMgr::GetEventValue" bin/mangosd > /tmp/playerbots-symbols && \
    grep -q RandomPlayerbotMgr.cpp /tmp/playerbots-symbols
# Retain the base image's wrappers, working directory and runtime command.
