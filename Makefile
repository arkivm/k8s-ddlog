
POLICY := kube_policy.dl

all:
	ddlog -i ${POLICY}
	cargo b --release
