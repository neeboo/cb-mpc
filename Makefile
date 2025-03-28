DOCKER_IMAGE_NAME := cb-mpc
RUN_CMD := bash -c
.DEFAULT_GOAL := ghas
CMAKE_NCORES := $(shell \
	if [ -n "$${ARG_CMAKE_NCORES}" ]; then \
		echo "$${ARG_CMAKE_NCORES}"; \
    elif command -v nproc >/dev/null 2>&1; then \
        nproc; \
    elif [ "$$(uname)" = "Darwin" ]; then \
        sysctl -n hw.ncpu; \
    fi)

TEST_NCORES := $(shell \
	if [ -n "$${ARG_TEST_NCORES}" ]; then \
		echo "$${ARG_TEST_NCORES}"; \
    elif command -v nproc >/dev/null 2>&1; then \
        nproc; \
    elif [ "$$(uname)" = "Darwin" ]; then \
        sysctl -n hw.ncpu; \
    fi)
TEST_REPEAT ?= 1

.PHONY: ghas
ghas: submodules openssl-linux build

.PHONY: submodules
submodules:
	git submodule update --init --recursive

.PHONY: openssl-linux
openssl-linux:
	${RUN_CMD} 'bash ./scripts/openssl/build-static-openssl-linux.sh'
	${RUN_CMD} 'mkdir -p /usr/local/lib64'
	${RUN_CMD} 'mkdir -p /usr/local/lib'
	${RUN_CMD} 'mkdir -p /usr/local/include'

.PHONY: docker-run
docker-run:
	@echo "To run inside docker, you can run do the following"
	@echo "make image"
	@echo "docker run -it --rm -v $(shell pwd):/code -t ${DOCKER_IMAGE_NAME} bash -c 'make XXX'"

.PHONY: image
image:
	docker build -t ${DOCKER_IMAGE_NAME} .

.PHONY: lint-fix
lint-fix:
	find src/ -name '*.cpp' -o -name '*.h' | xargs clang-format -i
	find tests/ -name '*.cpp' -o -name '*.h' | xargs clang-format -i

.PHONY: lint
lint:
	find src/ -name '*.cpp' -o -name '*.h' | xargs clang-format -n 2>&1 | grep -q "^" && exit 1 || exit 0
	find tests/ -name '*.cpp' -o -name '*.h' | xargs clang-format -n 2>&1 | grep -q "^" && exit 1 || exit 0

.PHONY: build
build: BUILD_TYPE = Release# (Release/Debug/RelWithDebInfo)
build:
	${RUN_CMD} 'cmake -B build/$(BUILD_TYPE) -DCMAKE_BUILD_TYPE=$(BUILD_TYPE) -DBUILD_TESTS=ON && \
	cmake --build build/$(BUILD_TYPE) -- -j$(CMAKE_NCORES)'

.PHONY: build-no-test
build-no-test: BUILD_TYPE = Release# (Release/Debug/RelWithDebInfo)
build-no-test:
	${RUN_CMD} \
	'cmake -B build/$(BUILD_TYPE) -DCMAKE_BUILD_TYPE=$(BUILD_TYPE) -DBUILD_TESTS=OFF && \
	cmake --build build/$(BUILD_TYPE) -- -j$(CMAKE_NCORES)'

.PHONY: build-with-dudect
build-with-dudect: BUILD_TYPE = Release# (Release/Debug/RelWithDebInfo)
build-with-dudect:
	${RUN_CMD} 'cmake -B build/$(BUILD_TYPE) -DCMAKE_BUILD_TYPE=$(BUILD_TYPE) -DBUILD_TESTS=ON -DBUILD_DUDECT=ON && \
	cmake --build build/$(BUILD_TYPE) -- -j$(CMAKE_NCORES)'

.PHONY: test # e.g. make test filter=ED25519
test: BUILD_TYPE = Release# (Release/Debug/RelWithDebInfo)
test: TEST_LABEL =unit|integration# (unit|integration)
test:
	$(MAKE) build BUILD_TYPE=$(BUILD_TYPE)
	${RUN_CMD} \
	'ctest --output-on-failure --repeat until-fail:$(TEST_REPEAT) -j$(TEST_NCORES) --test-dir build/$(BUILD_TYPE) \
	$(if $(TEST_LABEL),-L "$(TEST_LABEL)") \
	$(if $(filter),-R $(filter))'

.PHONY: dev
dev:
	$(MAKE) test BUILD_TYPE=Debug TEST_LABEL=integration

.PHONY: full-test
full-test:
	$(MAKE) test BUILD_TYPE=Debug TEST_LABEL=unit

.PHONY: dudect
dudect:
	$(MAKE) build-with-dudect BUILD_TYPE=Release
	${RUN_CMD} \
	'ctest --output-on-failure --repeat until-fail:$(TEST_REPEAT) -j1 --test-dir build/Release \
	-L "dudect" \
	-E DUDECT_VT \
	$(if $(filter),-R $(filter))'

.PHONY: clean
clean:
	${RUN_CMD} 'rm -rf build'
	${RUN_CMD} 'rm -rf lib'

### Install the C++ library to local (this is necessary for demo and benchmark)
.PHONY: install
install:
	${RUN_CMD} 'scripts/install.sh'
	${RUN_CMD} 'ln -sf /usr/local/lib /usr/local/lib64'

.PHONY: uninstall
uninstall:
	${RUN_CMD} 'rm -rf /usr/local/opt/cbmpc'

### Demos
.PHONY: demos
demos:
	${RUN_CMD} 'bash ./scripts/run-demos.sh --run-all'

.PHONY: clean-demos
clean-demos:
	${RUN_CMD} 'bash ./scripts/run-demos.sh --clean'

### Benchmarks
include tools/benchmark/benchmark.makefile

.PHONY: bench
bench:
	$(MAKE) benchmark-build
	$(MAKE) benchmark-run unit=us

.PHONY: clean-bench
clean-bench:
	$(MAKE) bench-clean

.PHONY: sanity-check
sanity-check:
	set -e
	$(MAKE) build
	sudo $(MAKE) install
	docker run -it --rm -v $(shell pwd):/code -t ${DOCKER_IMAGE_NAME} bash -c 'make lint'
	$(MAKE) demos
	$(MAKE) test
	$(MAKE) benchmark-build
	$(MAKE) dudect filter=NON_EXISTING_TEST
