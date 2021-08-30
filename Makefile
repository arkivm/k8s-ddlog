
POLICY_NAME := kube_policy
POLICY := $(POLICY_NAME).dl

all:
	ddlog -i ${POLICY}
	cargo b --release

.PHONY=clean

clean:
	cargo clean
	@echo "Removing generated _ddlog"
	@rm -rf $(POLICY_NAME)_ddlog
