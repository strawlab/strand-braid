from __future__ import print_function
import adskalman.adskalman as adskalman
import pandas as pd
import sys
import numpy as np
import read_detection_csv
import os

def print(*args):
    pass

class TrackedObject:
    def __init__(self,dt,obj_id):
        self._dt = dt
        print('birth %d'%obj_id)
        self.obj_id = obj_id

    def _init_kfilt(self,xy):
        dt = self._dt
        # Create a 4-dimensional state space model:
        # (x, y, xvel, yvel).
        motion_model = np.array([[1.0, 0.0,  dt, 0.0],
                                [0.0, 1.0, 0.0,  dt],
                                [0.0, 0.0, 1.0, 0.0],
                                [0.0, 0.0, 0.0, 1.0]])

        # See http://webee.technion.ac.il/people/shimkin/Estimation09/ch8_target.pdf
        covar_scale = 10.0
        T33 = dt**3/3.0
        T22 = dt**2/2.0
        T   = dt
        motion_noise_covariance = covar_scale*np.array([[T33, 0.0, T22, 0.0],
                                            [0.0, T33, 0.0, T22],
                                            [T22, 0.0, T, 0.0],
                                            [0.0, T22, 0.0, T]])

        # Create observation model. We only observe the position.
        observation_model = np.array([[1.0, 0.0, 0.0, 0.0],
                                    [0.0, 1.0, 0.0, 0.0]])
        observation_noise_covariance = np.array([[0.01, 0.0],
                                                [0.0, 0.01]])

        self.data_association_distance_threshold_meters = 0.005 # 5 mm

        # My attempt to make this dt invariant. (For dt=0.015 I solved to
        # find covar_factor such that the parameter equaled an old value
        # that "worked for me".)

        # threshold to kill ongoing trajectory
        max_covar_factor = 7500.0
        self.max_position_covarance = max_covar_factor * T**3 / 3.0

        # threshold to save entire trajectory to disk
        covar_factor = 7111.0
        self.median_covar_scalar_threshold = covar_factor * T**3 / 3.0

        self.minimum_excursion_distance = 0.002 # 2mm

        F = motion_model
        H = observation_model
        Q = motion_noise_covariance
        R = observation_noise_covariance
        initx = np.array([xy[0],xy[1],0.0,0.0])
        initV = 0.1*np.eye(4)
        self._kfilt = adskalman.KalmanFilter(F, H, Q, R, initx, initV)

    def handle_birth(self,frame,observation):
        print('birth of obj %d on with data frame %d: %s'%(self.obj_id, frame, observation))
        self._init_kfilt(observation)
        self.last_frame = frame
        xfilt_i, Vfilt_i = self._kfilt.step(y=observation, isinitial=True)
        self.state_estimates = [ xfilt_i ]
        self.covariance_estimates = [ Vfilt_i ]
        self.observations = [observation]
        self.frames = [frame]
        self._last_observation_frame = frame

    def data_association_and_kalman_update(self, frame, orig_observations):
        # handle any potentially missing frames
        for missing_frame_number in range(self.last_frame+1, frame):
            # Perform kalman update with no observation.
            xfilt_i, Vfilt_i = self._kfilt.step(y=None)
            self.state_estimates.append( xfilt_i )
            self.covariance_estimates.append( Vfilt_i )
            self.observations.append((np.nan,np.nan))
            self.frames.append(missing_frame_number)
            self.last_frame = frame
            # Check if we have met termination criteria.
            is_done = self._check_is_done(frame)
            if is_done:
                return orig_observations, is_done

        # create prior estimate for this frame
        state_prior, covariance_prior = self._kfilt.step1__calculate_a_priori()
        predicted = np.dot(self._kfilt.C, state_prior)

        unclaimed_observations = []
        for observation in orig_observations:
            distance = np.sqrt(np.sum((predicted-observation)**2))
            if distance <= self.data_association_distance_threshold_meters:
                # use this observation
                xfilt_i, Vfilt_i = self._kfilt.step2__calculate_a_posteri(xhatminus=state_prior,
                    Pminus=covariance_prior, y=observation)
                self.state_estimates.append( xfilt_i )
                self.covariance_estimates.append( Vfilt_i )
                self.observations.append(observation)
                self.frames.append(frame)
                self._last_observation_frame = frame
                break # we got one observation, cannot consume more
            else:
                print('obj %d not using this observation'%self.obj_id)
                unclaimed_observations.append(observation)
        is_done = self._check_is_done(frame)
        return unclaimed_observations, is_done

    def _check_is_done(self,frame):
        # calculate a single value for uncertainty
        P = self.covariance_estimates[-1]
        position_coveriance_scalar = np.sqrt( P[0,0]**2 + P[1,1]**2 )
        if position_coveriance_scalar > self.max_position_covarance:
            print('killing obj_id %d on frame %d (%s > %s)'% (self.obj_id,frame,position_coveriance_scalar,self.max_position_covarance))
            is_done = True
        else:
            is_done = False
        return is_done

class Tracker:
    def __init__(self,dt,results_filename,meters_to_pixels):
        self._results_fd = open(results_filename,mode='w')
        self._results_fd.write('# dt: %s\n'%dt)
        self._results_fd.write('frame,obj_id,pos_x,pos_y,vel_x,vel_y,P00,P11,P22,P33,obs_x,obs_y,pos_x_pix,pos_y_pix\n')
        self._dt = dt
        self._objects = []
        self.next_obj_id = 0
        self.Ainv = meters_to_pixels
    def handle_frame_data(self,frame,observations):
        assert type(frame)==int
        unclaimed_observations = observations[:] # copy list
        # perform nearest-neighbor data association
        living_objects = []
        while len(self._objects):
            obj = self._objects.pop()
            unclaimed_observations, is_done = obj.data_association_and_kalman_update(frame,unclaimed_observations)
            if is_done:
                self._save_data(obj)
                print('death %d'%obj.obj_id)
            else:
                living_objects.append( obj )
        self._objects = living_objects

        # Handle observations which are not associated with an object by giving 'birth'
        # to new object.
        while len(unclaimed_observations):
            this_observation = unclaimed_observations.pop()
            new_obj = TrackedObject(self._dt, self.next_obj_id)
            self.next_obj_id += 1
            new_obj.handle_birth(frame,this_observation)
            self._objects.append(new_obj)
    def close(self):
        while len(self._objects):
            obj = self._objects.pop()
            self._save_data(obj)
    def _save_data(self,obj):

        frames = np.array(obj.frames)
        invalid_cond = frames > obj._last_observation_frame
        last_idxs = np.nonzero(invalid_cond)
        last_idxs0 = last_idxs[0]
        if len(last_idxs0) == 0:
            last_idx = len(obj.frames)
        else:
            last_idx = last_idxs0[0]

        obj.frames = obj.frames[:last_idx]
        obj.state_estimates = obj.state_estimates[:last_idx]
        obj.covariance_estimates = obj.covariance_estimates[:last_idx]
        obj.observations = obj.observations[:last_idx]

        print("obj %d: %d frames with estimates" %(obj.obj_id, len(obj.state_estimates)))
        obj_id = obj.obj_id
        state_arr = np.array(obj.state_estimates)

        xy_meters = state_arr[:,:2].T
        h = np.ones_like(xy_meters[0,:])
        xy_meters_homog = np.array((xy_meters[0,:], xy_meters[1,:], h))
        assert xy_meters_homog.shape == (3, len(obj.state_estimates))
        xy_pixels_homog = np.dot(self.Ainv, xy_meters_homog)
        xy_pixels = xy_pixels_homog[:2]/xy_pixels_homog[2]

        x_meters = xy_meters[0,:]
        y_meters = xy_meters[1,:]
        # There is probably a smarter way to do this...
        x_excursion = np.max(x_meters) - np.min(x_meters)
        y_excursion = np.max(y_meters) - np.min(y_meters)
        excursion_distance = np.max(x_excursion, y_excursion)

        covar_scalars = []
        for P in obj.covariance_estimates:
            covar_scalars.append( np.sqrt( P[0,0]**2 + P[1,1]**2 ) )
        median_covar_scalar = np.median(covar_scalars)
        print('frame start, frame end, median_covar_scalar',obj.frames[0],obj.frames[-1], median_covar_scalar)

        # Only save data if mean error less than some amount.
        if not median_covar_scalar <= obj.median_covar_scalar_threshold:
            print('***** not saving obj_id %d: median_covar_scalar (%s) exceeds threshold' % (obj_id,median_covar_scalar))
            return

        # Avoid saving stationary "trajectories" by only saving trajectories
        # with some movement.
        if not excursion_distance > obj.minimum_excursion_distance:
            print('***** not saving obj_id %d: excursion distance (%s) does not exceed threshold' % (obj_id, excursion_distance))
            return

        for frame, state, covar, obs, xy_pix in zip(obj.frames, obj.state_estimates, obj.covariance_estimates, obj.observations, xy_pixels.T):
            self._results_fd.write('%d,%d,%f,%f,%f,%f,%f,%f,%f,%f,%f,%f,%f,%f\n' %
                (frame, obj_id,
                    state[0], state[1], state[2], state[3],
                    covar[0,0], covar[1,1], covar[2,2], covar[3,3],
                    obs[0], obs[1],
                    xy_pix[0], xy_pix[1]))

def calc_dt(df):
    f0 = df['frame'].iloc[0]
    t0 = df['timestamp'].iloc[0]
    f1 = df['frame'].iloc[-1]
    t1 = df['timestamp'].iloc[-1]
    dur_secs = t1-t0
    dur_frames = f1-f0
    dt = dur_secs/dur_frames
    return dt

def main():
    filename = sys.argv[1]
    df = read_detection_csv.get_df(filename)
    A, Ainv = read_detection_csv.get_A_Ainv(filename)
    dt = calc_dt(df)

    frame = None
    observations = []
    results_filename = os.path.splitext(filename)[0] + '.kalmanized.csv'
    tracker = Tracker(dt, results_filename, meters_to_pixels = Ainv)
    for _,row in df.iterrows():
        # print('======= new row\n',row)
        if frame is None:
            # first frame
            frame = int(row['frame'])
            frame0 = frame
            # print('-------------- first frame')

        if row['frame'] > frame:
            # next frame
            tracker.handle_frame_data(frame,observations)
            # print('-------------- frame advance')
            observations = []
            frame = int(row['frame'])

        # print('-------------- within frame')
        observations.append( (row['x'], row['y'] ))
        # current frame

        # if frame >= frame0+100:
        #     break
    tracker.handle_frame_data(frame,observations)
    tracker.close()

if __name__=='__main__':
    main()
