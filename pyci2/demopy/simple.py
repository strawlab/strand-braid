import pyci2.core as cam_iface

import numpy as nx
import time, sys, os
from optparse import OptionParser

# for save mode:
import motmot.FlyMovieFormat.FlyMovieFormat as FlyMovieFormat
import Queue
import threading

def main():
    usage = '%prog [options]'

    parser = OptionParser(usage)

    parser.add_option("--mode-num", type="int",default = None,
                      help="mode number")

    parser.add_option("--frames", type="int",
                      help="number of frames (default = infinite)",
                      default = None)

    parser.add_option("--save", action='store_true',
                      help="save frames to .fmf")

    parser.add_option("--trigger-mode", type="int",
                      help="set trigger mode",
                      default=None, dest='trigger_mode')

    parser.add_option("--roi", type="string",
                      help="set camera region of interest (left,bottom,width,height)",
                      default=None)

    (options, args) = parser.parse_args()

    if options.roi is not None:
        try:
            options.roi = tuple(map(int,options.roi.split(',')))
        except:
            print >> sys.stderr, "--roi option could not be understood. Use 4 "\
                "comma-separated integers (L,B,W,H)"
        assert len(options.roi)==4,"ROI must have 4 components (L,B,W,H)"

    print 'options.mode_num',options.mode_num

    doit(mode_num=options.mode_num,
         save=options.save,
         max_frames = options.frames,
         trigger_mode=options.trigger_mode,
         roi=options.roi)

def save_func( fly_movie, save_queue ):
    while 1:
        fnt = save_queue.get()
        frame,timestamp = fnt
        fly_movie.add_frame(frame,timestamp)

def doit(device_num=0,
         mode_num=None,
         num_buffers=30,
         save=False,
         max_frames=None,
         trigger_mode=None,
         roi=None,
         ):
    num_modes = cam_iface.get_num_modes(device_num)
    for this_mode_num in range(num_modes):
        mode_str = cam_iface.get_mode_string(device_num,this_mode_num)
        print 'mode %d: %s'%(this_mode_num,mode_str)
        if mode_num is None:
            if 'DC1394_VIDEO_MODE_FORMAT7_0' in mode_str and 'MONO8' in mode_str:
                mode_num=this_mode_num

    if mode_num is None:
        mode_num=0
    print 'choosing mode %d'%(mode_num,)

    cam = cam_iface.Camera(device_num,num_buffers,mode_num)

    if save:
        format = cam.get_pixel_coding()
        depth = cam.get_pixel_depth()
        filename = time.strftime( 'simple%Y%m%d_%H%M%S.fmf' )
        fly_movie = FlyMovieFormat.FlyMovieSaver(filename,
                                                 version=3,
                                                 format=format,
                                                 bits_per_pixel=depth,
                                                 )
        save_queue = Queue.Queue()
        save_thread = threading.Thread( target=save_func, args=(fly_movie,save_queue))
        save_thread.setDaemon(True)
        save_thread.start()

    num_props = cam.get_num_camera_properties()
    #for i in range(num_props):
    #    print "property %d: %s"%(i,str(cam.get_camera_property_info(i)))

    n_trigger_modes = cam.get_num_trigger_modes()
    print "Trigger modes:"
    for i in range(n_trigger_modes):
        print ' %d: %s'%(i,cam.get_trigger_mode_string(i))
    if trigger_mode is not None:
        cam.set_trigger_mode_number( trigger_mode )
    print 'Using trigger mode %d'%(cam.get_trigger_mode_number())

    cam.start_camera()
    if roi is not None:
        cam.set_frame_roi( *roi )
        actual_roi = cam.get_frame_roi()
        if roi != actual_roi:
            raise ValueError("could not set ROI. Actual ROI is %s."%(actual_roi,))
    frametick = 0
    framecount = 0
    last_fps_print = time.time()
    last_fno = None
    while 1:
        try:
            buf = nx.asarray(cam.grab_next_frame_blocking())
        except cam_iface.FrameDataMissing:
            sys.stdout.write('M')
            sys.stdout.flush()
            continue
        except cam_iface.FrameSystemCallInterruption:
            sys.stdout.write('I')
            sys.stdout.flush()
            continue

        timestamp = cam.get_last_timestamp()

        fno = cam.get_last_framenumber()
        if last_fno is not None:
            skip = (fno-last_fno)-1
            if skip != 0:
                print 'WARNING: skipped %d frames'%skip
    ##    if frametick==50:
    ##        print 'sleeping'
    ##        time.sleep(10.0)
    ##        print 'wake'
        last_fno=fno
        now = time.time()
        sys.stdout.write('.')
        sys.stdout.flush()
        frametick += 1
        framecount += 1

        t_diff = now-last_fps_print
        if t_diff > 5.0:
            fps = frametick/t_diff
            print "%.1f fps"%fps
            last_fps_print = now
            frametick = 0

        if save:
            save_queue.put( (buf,timestamp) )

        if max_frames:
            if framecount >= max_frames:
                break

if __name__=='__main__':
    main()
