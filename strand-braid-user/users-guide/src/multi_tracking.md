# 3D Tracking multiple objects with Braid

It is possible to use Braid to track 2 or more objects. To do so, please make sure the parameter `max_num_points` in the Object Detection configuration is set to at least the number of objects that should be tracked before starting to record.

To maintain a correct indentification of identity over time, it might be necessary to fine-tune the tracking parameters to get the best performance possible. The data association algorithm in Braid is also based on the assumption of independence between trajectories, however, previous experiments showed good performance on the tracking of interacting animals. 
