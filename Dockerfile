FROM gcr.io/distroless/static-debian12:nonroot@sha256:afa5c872c891853ca7fcf1f12c3edb23f7eeef36189728842dd51042ff57f7ab
# The release workflow places the attested, statically linked binary (the one in the release
# tarball) at ./rubrica and builds this image from it; nothing is compiled here. The base
# image has no shell and no package manager, and runs as a non-root user. Nothing writes to
# the root filesystem, so run it with `docker run --read-only`.
COPY rubrica /usr/local/bin/rubrica
USER nonroot:nonroot
ENTRYPOINT ["/usr/local/bin/rubrica"]
