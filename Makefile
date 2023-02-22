ADDR=0.0.0.0:3000
BUILD_CMD=cargo build --release --bin=

build-0:
	$(BUILD_CMD)smoke-test
	cp target/release/smoke-test binaries/smoke-test

binaries/smoke-test: build-0

0: binaries/smoke-test
	binaries/smoke-test $(ADDR)

build-1:
	$(BUILD_CMD)prime-time
	cp target/release/prime-time binaries/prime-time

binaries/prime-time: build-1

1: binaries/prime-time
	binaries/prime-time $(ADDR)

build-2:
	$(BUILD_CMD)means-2-end
	cp target/release/means-2-end binaries/means-2-end

binaries/means-2-end: build-2

2: binaries/means-2-end
	binaries/means-2-end $(ADDR)

build-3:
	$(BUILD_CMD)budget-chat
	cp target/release/budget-chat binaries/budget-chat

binaries/budget-chat: build-3

3: binaries/budget-chat
	binaries/budget-chat $(ADDR)

build-4:
	$(BUILD_CMD)unusual-db
	cp target/release/unusual-db binaries/unusual-db

binaries/unusual-db: build-4

4: binaries/unusual-db
	binaries/unusual-db $(ADDR)

build-5:
	$(BUILD_CMD)mob-in-middle
	cp target/release/mob-in-middle binaries/mob-in-middle

binaries/mob-in-middle: build-5

5: binaries/mob-in-middle
	binaries/mob-in-middle $(ADDR)

build-6:
	$(BUILD_CMD)speed-daemon
	cp target/release/speed-daemon binaries/speed-daemon

binaries/speed-daemon: build-6

6: binaries/speed-daemon
	binaries/speed-daemon $(ADDR)
