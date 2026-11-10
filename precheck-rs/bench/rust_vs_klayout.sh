#!/bin/bash
# Same-machine comparison: precheck-rs vs the real KLayout deck (run inside the gf-lean image).
# usage: rust_vs_klayout.sh <precheck-rs binary> <outdir>
set -u
BIN=$1; OUT=$2; mkdir -p "$OUT"
HERE=$(cd "$(dirname "$0")/.." && pwd)
export PATH="${PRECHECK_TOOLS:+$PRECHECK_TOOLS:}/opt/precheck/bin:$PATH"
PY=/opt/precheck/bin/python
T=$HERE/testdata
DRC=/pdk-precheck/gf180mcuD/libs.tech/klayout/tech/drc
# copy of the deck whose logger does not spawn `pmap` for every line
cp -r $DRC /tmp/drc-nolog
sed -i 's/`pmap #{Process.pid} | tail -1`\[10, 40\].strip/"-"/' /tmp/drc-nolog/gf180mcu.drc
grep -n "Memory Usage" /tmp/drc-nolog/gf180mcu.drc
nproc; grep -m1 "model name" /proc/cpuinfo; klayout -v; $PY -c "import klayout.db as k; print('klayout.db', k.__version__)"
echo "name,gds,seconds" > $OUT/times.csv
now() { date +%s.%N; }
rec() { echo "$1,$2,$($PY -c "print(round($4-$3,3))")" | tee -a $OUT/times.csv; }

kl() { # name gds deckdir decks extra...
  local name=$1 gds=$2 dir=$3 decks=$4; shift 4
  local rd=(); for a in "$@"; do rd+=(-rd "$a"); done
  local t0=$(now)
  klayout -b -r $dir/gf180mcu.drc -rd input=$T/$gds.gds -rd topcell=tt_um_example -rd variant=gf180mcuD -rd run_mode=deep -rd threads=1 \
     -rd decks="$decks" -rd report=$OUT/$name.$gds.xml "${rd[@]}" 2>&1 | $PY $HERE/bench/stamp.py > $OUT/$name.$gds.log
  rec $name $gds $t0 $(now)
}
MAIN="all,-density,-antenna"
SUB="via,contact,metal,metaltop,offgrid"
ONLY="geom,M,V,CO,metaltop"

# --- Rust
for g in clean injected stress; do
  for th in 1 4; do
    t0=$(now); $BIN $T/$g.gds --top tt_um_example --threads $th --out $OUT/rs_t$th.$g.json --timing > $OUT/rs_t$th.$g.txt; rec rust_t$th $g $t0 $(now)
  done
done
for th in 1 2 4; do $BIN $T/clean.gds --top tt_um_example --threads $th --repeat 7 | grep ^wall | sed "s/^/rust clean /" | tee -a $OUT/rust_repeat.txt; done
for th in 1 4; do $BIN $T/clean.gds --top tt_um_example --threads $th --repeat 7 --only $ONLY | grep ^wall | sed "s/^/rust clean subset /" | tee -a $OUT/rust_repeat.txt; done
for i in 1 2 3; do t0=$(now); $BIN $T/clean.gds --top tt_um_example > /dev/null; rec rust_process_t1 clean $t0 $(now); done

# --- KLayout, real deck
for i in 1 2; do kl base_main_r$i clean $DRC "$MAIN"; done
for i in 1 2 3; do kl nolog_main_r$i clean /tmp/drc-nolog "$MAIN"; done
kl nolog_main_verbose clean /tmp/drc-nolog "$MAIN" verbose=true
kl base_antenna clean $DRC antenna
for i in 1 2; do kl nolog_antenna_r$i clean /tmp/drc-nolog antenna; done
kl base_subset clean $DRC "$SUB"
for i in 1 2 3; do kl nolog_subset_r$i clean /tmp/drc-nolog "$SUB"; done
kl nolog_subset_verbose clean /tmp/drc-nolog "$SUB" verbose=true
kl base_main stress $DRC "$MAIN"
kl base_main injected $DRC "$MAIN"
kl nolog_main_flat stress /tmp/drc-nolog "$MAIN" run_mode=flat
kl nolog_main_t4 clean /tmp/drc-nolog "$MAIN" threads=4

# --- KLayout engine through the Python API (same rules as the Rust slice, deep mode, no Ruby deck)
for g in clean stress; do
  t0=$(now); $PY $HERE/ref/kl_slice.py $T/$g.gds $OUT/klpy_deep.$g.json --deep | tee $OUT/klpy_deep.$g.txt; rec klpy_deep $g $t0 $(now)
done

# --- correctness: Rust markers vs the real deck
for g in clean injected stress; do
  $PY $HERE/bench/lyrdb2json.py $OUT/base_main.$g.xml $OUT/deck.$g.json 2>/dev/null || $PY $HERE/bench/lyrdb2json.py $OUT/base_main_r1.$g.xml $OUT/deck.$g.json
  for th in 1 4; do echo "== rust t$th vs deck (deep): $g"; $PY $HERE/bench/compare.py $OUT/rs_t$th.$g.json $OUT/deck.$g.json -v | cut -c1-600 | tee -a $OUT/compare.txt; done
done
$PY $HERE/bench/lyrdb2json.py $OUT/nolog_main_flat.stress.xml $OUT/deck_flat.stress.json
echo "== deck flat vs deck deep: stress"; $PY $HERE/bench/compare.py $OUT/deck_flat.stress.json $OUT/deck.stress.json | cut -c1-300 | tee -a $OUT/compare.txt
echo "== klayout.db deep (python) vs deck deep: stress"; $PY $HERE/bench/compare.py $OUT/klpy_deep.stress.json $OUT/deck.stress.json | cut -c1-300 | tee -a $OUT/compare.txt

# --- whole precheck.py (tt-support-tools), as shipped and with the PDK copy whose deck logger does not spawn pmap
if [ -n "${PRECHECK_DIR:-}" ] && [ -d "$PRECHECK_DIR" ]; then
  cp -a /pdk-precheck /tmp/pdk-nolog
  sed -i 's/`pmap #{Process.pid} | tail -1`\[10, 40\].strip/"-"/' /tmp/pdk-nolog/gf180mcuD/libs.tech/klayout/tech/drc/gf180mcu.drc
  grep -c 'pmap' /tmp/pdk-nolog/gf180mcuD/libs.tech/klayout/tech/drc/gf180mcu.drc
  for v in base nolog base nolog; do
    if [ $v = base ]; then export PDK_ROOT=/pdk-precheck; else export PDK_ROOT=/tmp/pdk-nolog; fi
    t0=$(now)
    (cd $PRECHECK_DIR && $PY precheck.py --gds $SUBMISSION/tt_submission/tt_um_example.gds --tech gf180mcuD 2>&1 | $PY $HERE/bench/stamp.py > $OUT/precheck_$v.log; echo "rc=${PIPESTATUS[0]}" >> $OUT/precheck_$v.log)
    rec precheck_$v submission $t0 $(now)
    grep -E "INFO: Running|Precheck passed|rc=|Error|FAIL" $OUT/precheck_$v.log | cut -c1-110
  done
fi
cat $OUT/times.csv
