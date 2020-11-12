# flydra2

Develop `flydra2-mainbrain` with:

    # On Mac (without ROS):
    cargo run --features "posix_sched_fifo" -- -r ../braid-offline/test_data/calibration.xml -o /tmp/mb2

    # On Ubuntu 16.04 amd64 (with ROS):
    export ROSRUST_MSG_PATH=`pwd`/../_submodules:`pwd`/../_submodules/ros_comm_msgs:`pwd`/../_submodules/common_msgs:`pwd`/../image-tracker
    cargo run --features "ros" -- -r ../braid-offline/test_data/calibration.xml -o /tmp/mb2
