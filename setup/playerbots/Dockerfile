# syntax=docker/dockerfile:1
ARG BASE_IMAGE
FROM ${BASE_IMAGE} AS runtime
USER root
# Refresh runtime packages when the installed compiler/dependencies change.
ARG TOOLCHAIN_ID
# gdb plus unstripped binaries permit a backtrace without host core-dump setup.
RUN apt-get update && apt-get install -y --no-install-recommends gdb && rm -rf /var/lib/apt/lists/*
COPY package/opt/vmangos /opt/vmangos
RUN ldd /opt/vmangos/bin/mangosd > /tmp/playerbots-libraries && \
    ldd /opt/vmangos/bin/realmd >> /tmp/playerbots-libraries && \
    ! grep -q 'not found' /tmp/playerbots-libraries && \
    /opt/vmangos/bin/mangosd --version && /opt/vmangos/bin/realmd --version && \
    cd /opt/vmangos && sha256sum -c share/playerbots/binaries.sha256 && \
    gdb --batch -ex "info line RandomPlayerbotMgr::GetEventValue" bin/mangosd > /tmp/playerbots-symbols && \
    grep -q RandomPlayerbotMgr.cpp /tmp/playerbots-symbols
# Retain the base image's wrappers, working directory and runtime command.
