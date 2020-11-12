import re
import sys

re_line = re.compile(r'^\W*(?P<command>[A-Za-z0-9/_:-]+)\W+(?P<pid>[0-9]+)\W+\[(?P<cpu>[0-9]+)\]\W+(?P<timestamp>[0-9.]+):\W+(?P<event>[a-z:_]+): (?P<text>.+)$')
re_wakeup = re.compile(r'^comm=(?P<comm>[A-Za-z0-9/_:-]+) pid=(?P<pid>[0-9]+) prio=(?P<prio>[0-9]+) target_cpu=(?P<target_cpu>[0-9]+)$')

parsers = {
    'sched:sched_switch':
        re.compile(r'^prev_comm=(?P<prev_comm>[A-Za-z0-9/_:-]+) prev_pid=(?P<prev_pid>[0-9]+) prev_prio=(?P<prev_prio>[0-9]+) prev_state=(?P<prev_state>[A-Z+]+) ==> next_comm=(?P<next_comm>[A-Za-z0-9/_:-]+) next_pid=(?P<next_pid>[0-9]+) next_prio=(?P<next_prio>[0-9]+)$'),
    'sched:sched_wakeup': re_wakeup,
    'sched:sched_wakeup_new': re_wakeup,
    'sched:sched_migrate_task':
        re.compile(r'^comm=(?P<comm>[A-Za-z0-9/_:-]+) pid=(?P<pid>[0-9]+) prio=(?P<prio>[0-9]+) orig_cpu=(?P<orig_cpu>[0-9]+) dest_cpu=(?P<dest_cpu>[0-9]+)$'),
    'sched:sched_stat_runtime': re.compile(r'^.*$'),
    'sched:sched_process_fork': re.compile(r'^.*$'),
}

# see https://stackoverflow.com/a/50768749

fname = sys.argv[1] # file recorded with e.g. `perf sched record -- sleep 10`
bad_time = sys.argv[2] # get bad time looking at `perf sched latency`

bad_time_f = float(bad_time)

follow = 'frame_process_t'
bad_range0 = bad_time_f - 0.2
bad_range1 = bad_time_f + 0.001

events = []
with open(fname,mode='r') as fd:
    for lineno, line in enumerate(fd.readlines()):
        if line.startswith('#'):
            continue
        gd = re_line.match(line).groupdict()
        timestamp_f = float(gd['timestamp'])
        gd['timestamp_f'] = timestamp_f
        gd['line'] = line.rstrip()

        if (bad_range0 < timestamp_f) and (timestamp_f < bad_range1):
            text = gd.pop('text')
            gd['event_data'] = parsers[gd['event']].match(text).groupdict()
            events.append( gd )
        elif timestamp_f > bad_range1:
            break

if 0:
    for gd in events:
        event = gd['event']
        if event == 'sched:sched_switch':
            if cpu=='005':
                print(gd['line'])

if 0:
    for gd in events:
        print(gd['line'])
    sys.exit(0)

if 0:
    print(len(events))

    for gd in events:
        event = gd['event']
        event_data = gd['event_data']
        found = False
        if event in ('sched:sched_wakeup', 'sched:sched_wakeup_new'):
            if event_data['comm'].startswith(follow):
                print( gd['line'])
                found = True
        elif event == 'sched:sched_switch':
            if event_data['next_comm'].startswith(follow):
                print(gd['line'])
                found = True

        if not found and follow in gd['line']:
            print('NOT FOUND IN: %r'% gd['line'])
