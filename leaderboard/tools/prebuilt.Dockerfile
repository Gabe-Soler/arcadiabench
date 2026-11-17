# An image around a coordinator binary that is already built: the build
# context holds one file, `coordinator`. The leaderboard's own checks use it
# for the baselines, built on the same Ubuntu release as this image.
FROM ubuntu:24.04
COPY coordinator /usr/local/bin/coordinator
USER 65534:65534
ENTRYPOINT ["/usr/local/bin/coordinator"]
