# Build the C example against the static library from this repository:
#   make            (builds target/release/libwarpzone.a first)
# Against a release download, set LIB=/path/to/libwarpzone.a INC=/path/to/include
ROOT ?= ../..
LIB  ?= $(ROOT)/target/release/libwarpzone.a
INC  ?= $(ROOT)/crates/capi/include
CC   ?= cc

echo: echo.c $(LIB)
	$(CC) -std=c11 -Wall -Wextra -O2 -D_POSIX_C_SOURCE=199309L -I$(INC) -o $@ echo.c $(LIB) -lpthread -ldl -lm

$(ROOT)/target/release/libwarpzone.a:
	cd $(ROOT) && cargo build --release -p warpzone-capi

clean:
	rm -f echo
