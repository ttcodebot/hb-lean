# Replays LibreLane's own OpenROAD step scripts in ONE process:
# the design and libraries are read once (first step) and written once (last step).
# Input: env REPLAY_STEPS = list of step directories, each with _proc_env and COMMANDS.
set ::replay_io 0
set ::replay_db 0
set ::replay_keep {PATH HOME PWD SHLVL _ REPLAY_STEPS REPLAY_OUT LD_LIBRARY_PATH TCLLIBPATH}

proc replay_load_env {dir} {
    set f [open "$dir/_proc_env" rb]; set data [read $f]; close $f
    foreach kv [split $data "\0"] {
        set i [string first "=" $kv]
        if { $i <= 0 } { continue }
        set k [string range $kv 0 [expr {$i - 1}]]
        if { [lsearch -exact $::replay_keep $k] >= 0 } { continue }
        set ::env($k) [string range $kv [expr {$i + 1}] end]
    }
}

rename source _replay_source
proc source {args} {
    set f [lindex $args end]
    if { [string match "*/openroad/common/io.tcl" $f] } {
        if { $::replay_io } { return }
        uplevel #0 [list _replay_source $f]
        set ::replay_io 1
        rename read_current_odb _replay_read_current_odb
        proc read_current_odb {args} {
            if { !$::replay_db } { _replay_read_current_odb; set ::replay_db 1 }
        }
        proc exit_unless_gui {args} { return -code return }
        return
    }
    uplevel 1 [list _replay_source {*}$args]
}

set steps $::env(REPLAY_STEPS)
set n [llength $steps]
set i 0
set t_all [clock milliseconds]
foreach dir $steps {
    incr i
    replay_load_env $dir
    _replay_source $::env(_TCL_ENV_IN)
    if { $i < $n } {
        foreach k [array names ::env SAVE_*] { unset ::env($k) }
    } else {
        foreach k [array names ::env SAVE_*] {
            set ::env($k) [file join $::env(REPLAY_OUT) [file tail $::env($k)]]
        }
    }
    set f [open "$dir/COMMANDS"]; set cmd [lindex [split [string trim [read $f]] "\n"] 0]; close $f
    set script [lindex $cmd end]
    set t0 [clock milliseconds]
    if { [catch {uplevel #0 [list source $script]} err] } {
        puts "REPLAY_ERROR [file tail $dir]: $err"
        puts $::errorInfo
        exit 1
    }
    puts "REPLAY_STEP [file tail $dir] [expr {[clock milliseconds] - $t0}] ms"
}
puts "REPLAY_TOTAL [expr {[clock milliseconds] - $t_all}] ms"
