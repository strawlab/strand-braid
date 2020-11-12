target remote :3333
monitor tpiu config internal /tmp/itm.fifo uart off 8000000
monitor itm port 0 on
load
#tbreak cortex_m_rt::reset_handler
#monitor reset halt
continue