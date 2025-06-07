GCP_PROJECT ?= bridge-misc
PLATFORM ?= linux/amd64
CRATE = btc-relayer

docker-build:
	echo "Building for ${PLATFORM}"
	docker compose build

docker-push: docker-build
	TAG="$$(date +'%Y%m%d')-$$(git rev-parse --short HEAD)"; \
	IMAGE="europe-west4-docker.pkg.dev/${GCP_PROJECT}/omni-bridge-docker-images/${CRATE}:$${TAG}"; \
	docker tag "${CRATE}:latest" "$${IMAGE}" && \
	docker push "$${IMAGE}" && \
	echo "Pushed $${IMAGE} to Artifact Registry"