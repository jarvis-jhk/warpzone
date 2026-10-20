# Build the C example against the static library from this repository:
#   make            (builds target/release/libwarpzone.a first)
# Against a release download, set LIB=/path/to/libwarpzone.a INC=/path/to/include
ROOT ?= ../..
LIB  ?= $(ROOT)/target/release/libwarpzone.a
INC  ?= $(ROOT)/crates/capi/include
CC   ?= cc
# macOS: LDLIBS from the release's NATIVE_LIBS.txt (frameworks, -liconv ...)
LDLIBS ?= -lpthread -ldl -lm

echo: echo.c $(LIB)
	$(CC) -std=c11 -Wall -Wextra -O2 -D_POSIX_C_SOURCE=200809L -I$(INC) -o $@ echo.c $(LIB) $(LDLIBS)

$(ROOT)/target/release/libwarpzone.a:
	cd $(ROOT) && cargo build --release -p warpzone-capi

clean:
	rm -f echo
