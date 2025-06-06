MAKEFILE_DIR :=  $(shell dirname $(realpath $(lastword $(MAKEFILE_LIST))))
LINT_OPTIONS = -D warnings -D clippy::pedantic -A clippy::must_use_candidate -A clippy::used_underscore_binding -A clippy::needless_range_loop
NEAR_MANIFEST := $(MAKEFILE_DIR)/contract/Cargo.toml

FEATURES = bitcoin dogecoin litecoin zcash
FEATURES_TEST = bitcoin litecoin zcash

all: build clippy

build: $(addprefix build-,$(FEATURES))

clippy: $(addprefix clippy-,$(FEATURES))

fmt: $(addprefix fmt-,$(FEATURES))

test: $(addprefix test-,$(FEATURES))

$(foreach feature,$(FEATURES), \
	$(eval build-$(feature): ; \
		cargo near build reproducible-wasm --variant "$(feature)" --manifest-path $(NEAR_MANIFEST) && \
		mkdir -p res && mv ./contract/target/near/btc_light_client_contract.wasm ./res/$(feature).wasm \
	) \
)

$(foreach feature,$(FEATURES), \
  $(eval clippy-$(feature): ; cargo clippy --no-default-features --features "$(feature)" --manifest-path $(NEAR_MANIFEST) -- $(LINT_OPTIONS)) \
)

$(foreach feature,$(FEATURES), \
	$(eval fmt-$(feature): ; cargo fmt --all --check --manifest-path $(NEAR_MANIFEST)) \
)

$(foreach feature,$(FEATURES_TEST), \
	$(eval test-$(feature): ; cargo test --no-default-features --features "$(feature)" --manifest-path $(NEAR_MANIFEST)) \
)