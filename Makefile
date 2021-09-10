
POLICY_NAME := kube_policy
POLICY := $(POLICY_NAME).dl

DDLOG_LIB_PATH := /Users/vikram/research/differential-datalog/lib

all:
	ddlog -i ${POLICY} -L ${DDLOG_LIB_PATH}
	cargo b --release

.PHONY=clean

clean:
	cargo clean
	@echo "Removing generated _ddlog"
	@rm -rf $(POLICY_NAME)_ddlog
