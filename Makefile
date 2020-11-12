default:
	# no default

target/release/fview2: elm
	cd fview2 && touch build.rs && cargo build --features "posix_sched_fifo ros bundle_files backend_pyloncxx" --release

fview2/elm_frontend/build/index.html:
	cd fview2/elm_frontend && elm-app build

elm: fview2/elm_frontend/build/index.html

fview2: target/release/fview2

.PHONY: default elm fview2
