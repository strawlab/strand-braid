#include <sched.h>

int get_policy_SCHED_OTHER() { return SCHED_OTHER; }
int get_policy_SCHED_FIFO() { return SCHED_FIFO; }
int get_policy_SCHED_RR() { return SCHED_RR; }
