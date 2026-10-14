# Build and run against the same base image: never copy host-linked binaries in.
ARG BASE_IMAGE
FROM ${BASE_IMAGE} AS build
USER root
ARG BUILD_JOBS=4
RUN apt-get update && apt-get install -y --no-install-recommends \
    build-essential cmake ninja-build git libboost-dev libmysqlclient-dev \
    libssl-dev zlib1g-dev && rm -rf /var/lib/apt/lists/*
COPY source /build/source
RUN cmake -S /build/source -B /build/output -G Ninja \
    -DCMAKE_BUILD_TYPE=Release -DCMAKE_INSTALL_PREFIX=/opt/vmangos \
    -DCONF_DIR=/opt/vmangos/config -DBUILD_PLAYERBOTS=ON \
    -DUSE_SCRIPTS=ON -DUSE_PCH=ON -DPLAYERBOTS_USE_PCH=ON \
    -DBUILD_EXTRACTORS=OFF -DBUILD_FOR_HOST_CPU=OFF \
    -DENABLE_CPPTRACE=OFF -DDEBUG_SYMBOLS=ON \
    -DCMAKE_EXPORT_COMPILE_COMMANDS=ON && \
    cmake --build /build/output --parallel ${BUILD_JOBS} --target mangosd realmd && \
    DESTDIR=/package cmake --install /build/output
COPY provenance.json /package/opt/vmangos/share/playerbots/provenance.json
RUN cd /package/opt/vmangos && sha256sum bin/mangosd bin/realmd > share/playerbots/binaries.sha256 && \
    c++ --version > share/playerbots/compiler.txt

FROM ${BASE_IMAGE} AS runtime
USER root
# gdb plus unstripped binaries permit a backtrace without host core-dump setup.
RUN apt-get update && apt-get install -y --no-install-recommends gdb && rm -rf /var/lib/apt/lists/*
COPY --from=build /package/opt/vmangos /opt/vmangos
RUN ldd /opt/vmangos/bin/mangosd > /tmp/playerbots-libraries && \
    ldd /opt/vmangos/bin/realmd >> /tmp/playerbots-libraries && \
    ! grep -q 'not found' /tmp/playerbots-libraries && \
    /opt/vmangos/bin/mangosd --version && /opt/vmangos/bin/realmd --version && \
    cd /opt/vmangos && sha256sum -c share/playerbots/binaries.sha256
# Retain the base image's wrappers, working directory and runtime command.
