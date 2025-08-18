# DOCKER_TAG ?= rcore-tutorial-v3:latest
# .PHONY: docker build_docker fmt
	
# docker:
# 	docker run --rm -it -v ${PWD}:/mnt -w /mnt --name rcore-tutorial-v3 ${DOCKER_TAG} bash

# build_docker: 
# 	docker build -t ${DOCKER_TAG} --target build .

# TODO: 保留格式化功能
.PHONY: fmt
fmt:
	cd os; cargo fmt; cd ..