GCP_PROJECT ?= bridge-misc
PLATFORM ?= linux/amd64

docker-build-%:
	echo "Building $*-relayer for ${PLATFORM}"
	docker compose -f compose-$*.yaml build

docker-push-%: docker-build-%
	CRATE=$*-relayer; \
	TAG="$$(date +'%Y%m%d')-$$(git rev-parse --short HEAD)"; \
	IMAGE="europe-west4-docker.pkg.dev/${GCP_PROJECT}/omni-bridge-docker-images/$${CRATE}:$${TAG}"; \
	docker tag "$${CRATE}:latest" "$${IMAGE}" && \
	docker push "$${IMAGE}" && \
	echo "Pushed $${IMAGE} to Artifact Registry"