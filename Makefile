MAKEFILE_DIR :=  $(shell dirname $(realpath $(lastword $(MAKEFILE_LIST))))
LINT_OPTIONS = -D warnings -D clippy::pedantic -A clippy::missing_errors_doc -A clippy::must_use_candidate -A clippy::module_name_repetitions
NEAR_MANIFEST := $(MAKEFILE_DIR)/contract/Cargo.toml

FEATURES = bitcoin dogecoin litecoin zcash

all: build clippy

build: $(addprefix build-,$(FEATURES))

clippy: $(addprefix clippy-,$(FEATURES))

$(foreach feature,$(FEATURES), \
	$(eval build-$(feature): ; \
		cargo near build reproducible-wasm --variant "$(feature)" --manifest-path $(NEAR_MANIFEST) && \
		mv ./contract/target/near/btc_light_client_contract.wasm ./res/$(feature).wasm \
	) \
)

$(foreach feature,$(FEATURES), \
  $(eval clippy-$(feature): ; cargo clippy --no-default-features --features "$(feature)" --manifest-path $(NEAR_MANIFEST) -- $(LINT_OPTIONS)) \
)