#!/bin/bash
# Time the real gf180mcu KLayout deck in several configurations.
# usage: klayout_variants.sh <outdir>   (run inside the gf-lean image)
set -u
OUT=$1; mkdir -p "$OUT"
HERE=$(cd "$(dirname "$0")/.." && pwd)
export PATH="${PRECHECK_TOOLS:+$PRECHECK_TOOLS:}/opt/precheck/bin:$PATH"
DECK=/pdk-precheck/gf180mcuD/libs.tech/klayout/tech/drc/gf180mcu.drc
PY=/usr/local/bin/python3
REALPATH=$PATH
mkdir -p /tmp/stub && printf '#!/bin/sh\necho " total                0K"\n' > /tmp/stub/pmap && chmod +x /tmp/stub/pmap
echo "name,gds,seconds,rc" > "$OUT/times.csv"
nproc; grep -m1 "model name" /proc/cpuinfo; klayout -v

run() { # name gds stub(0/1) decks extra-rd...
  local name=$1 gds=$2 stub=$3 decks=$4; shift 4
  local rd=(); for a in "$@"; do rd+=(-rd "$a"); done
  if [ "$stub" = 1 ]; then export PATH=/tmp/stub:$REALPATH; else export PATH=$REALPATH; fi
  local t0=$(date +%s.%N)
  klayout -b -r $DECK -rd input=$HERE/testdata/$gds.gds -rd topcell=tt_um_example -rd variant=gf180mcuD \
     -rd decks="$decks" -rd report=$OUT/$name.$gds.xml "${rd[@]}" 2>&1 | $PY $HERE/bench/stamp.py > $OUT/$name.$gds.log
  local rc=${PIPESTATUS[0]}
  local t1=$(date +%s.%N)
  echo "$name,$gds,$(echo "$t1 - $t0" | bc -l 2>/dev/null || $PY -c "print($t1-$t0)"),$rc" | tee -a "$OUT/times.csv"
  export PATH=$REALPATH
}
MAIN="all,-density,-antenna"
# start-up cost of klayout itself
echo 'puts 1' > /tmp/empty.rb
for i in 1 2 3; do t0=$(date +%s.%N); klayout -b -r /tmp/empty.rb >/dev/null; t1=$(date +%s.%N); echo "startup,none,$($PY -c "print($t1-$t0)"),0" | tee -a $OUT/times.csv; done
for i in 1 2 3; do t0=$(date +%s.%N); pmap $$ >/dev/null; t1=$(date +%s.%N); echo "pmap_small,none,$($PY -c "print($t1-$t0)"),0" | tee -a $OUT/times.csv; done

for i in 1 2 3; do run base_deep_t1_r$i clean 0 "$MAIN" run_mode=deep threads=1; done
for i in 1 2 3; do run stub_deep_t1_r$i clean 1 "$MAIN" run_mode=deep threads=1; done
run stub_deep_t1_verbose clean 1 "$MAIN" run_mode=deep threads=1 verbose=true
for i in 1 2; do run stub_flat_t1_r$i clean 1 "$MAIN" run_mode=flat threads=1; done
run base_flat_t1 clean 0 "$MAIN" run_mode=flat threads=1
for i in 1 2; do run stub_deep_t4_r$i clean 1 "$MAIN" run_mode=deep threads=4; done
run stub_flat_t4 clean 1 "$MAIN" run_mode=flat threads=4
run stub_tiling_t1 clean 1 "$MAIN" run_mode=tiling threads=1
run stub_tiling_t4 clean 1 "$MAIN" run_mode=tiling threads=4
run stub_deep_w4 clean 1 "$MAIN" run_mode=deep threads=1 workers=4
run base_deep_w4 clean 0 "$MAIN" run_mode=deep threads=1 workers=4
for i in 1 2; do run base_antenna_r$i clean 0 antenna run_mode=deep threads=1; done
for i in 1 2; do run stub_antenna_r$i clean 1 antenna run_mode=deep threads=1; done
run stub_antenna_verbose clean 1 antenna run_mode=deep threads=1 verbose=true
run stub_antenna_flat clean 1 antenna run_mode=flat threads=1
# injected-violation GDS: reference results in deep (as precheck) and the alternatives
run base_deep_t1 injected 0 "$MAIN" run_mode=deep threads=1
run stub_flat_t1 injected 1 "$MAIN" run_mode=flat threads=1
run stub_deep_t4 injected 1 "$MAIN" run_mode=deep threads=4
run stub_tiling_t4 injected 1 "$MAIN" run_mode=tiling threads=4
run stub_deep_w4 injected 1 "$MAIN" run_mode=deep threads=1 workers=4
run stub_antenna injected 1 antenna run_mode=deep threads=1
cat "$OUT/times.csv"
