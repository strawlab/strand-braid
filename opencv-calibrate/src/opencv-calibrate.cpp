#include "opencv2/calib3d/calib3d.hpp"
#include "opencv2/imgproc/imgproc.hpp"

extern "C" {

struct cv_return_value_double {
    char is_cv_exception;
    char is_other_exception;
    double result;
};

struct cv_return_value_bool {
    char is_cv_exception;
    char is_other_exception;
    bool result;
};

struct cv_return_value_slice {
    char is_cv_exception;
    char is_other_exception;
    void* ptr;
    int num_elements;
};

struct cv_return_value_double calibrate_camera(
    int image_count,
    double const* const object_points,  // (1,total) CV_64FC3
    double const* const image_points,  // (1,total) CV_64FC2
    int const* const point_counts,  // (1,image_count) CV_32S
    int imgWidth,
    int imgHeight,
    double* camera_matrix, // (3,3) double
    double* distortion_coeffs,  // (5,1) double
    double* rotation_matrices,  // (imageCount,9) double
    double* translation_vectors  // (imageCount,3) double
){
    struct cv_return_value_double result = { 0, 0, 0.0 };

    try {
        int i, total = 0;
        for( i = 0; i < image_count; i++ ) {
            total += point_counts[i];
        }

        // Create C++ wrapper/view around externally allocated data.
        cv::Mat pointCounts(1, image_count, CV_32S, (void*)point_counts);
        cv::Mat cameraMatrix(3, 3, CV_64F, (void*)camera_matrix);
        cv::Mat distortionCoeffs(5, 1, CV_64F, (void*)distortion_coeffs);
        cv::Mat rotationMatrices(image_count, 9, CV_64F, (void*)rotation_matrices);
        cv::Mat translationVectors(image_count, 3, CV_64F, (void*)translation_vectors);

        // Copy external data into OpenCV data structures
        std::vector<std::vector<cv::Point3f> > obj_pts;
        std::vector<std::vector<cv::Point2f> > im_pts;

        int k = 0;
        for( i = 0; i < image_count; i++ ) {
            std::vector<cv::Point3f> obj_pts_inner;
            std::vector<cv::Point2f> im_pts_inner;
            for (int j=0; j<point_counts[i]; j++) {
                obj_pts_inner.push_back( cv::Point3f( object_points[k*3], object_points[k*3+1], object_points[k*3+2] ) );
                im_pts_inner.push_back( cv::Point2f( image_points[k*2], image_points[k*2+1] ) );
                k += 1;
            }
            obj_pts.push_back(obj_pts_inner);
            im_pts.push_back(im_pts_inner);
        }

        // cvCalibrateCamera2 detects size of distortionCoeffs matrix and sets
        // flags appropriately. Furthermore, we are trying to copy the behavior
        // of the ROS `camera_calibration` package `cameracalibrator.py` node (which uses
        // camera_calibration.calibrator.MonoCalibrator`) which sets flags
        // cv2.CALIB_FIX_K6 | cv2.CALIB_FIX_K5 | cv2.CALIB_FIX_K4 | cv2.CALIB_FIX_K3

        int calibFlags = cv::CALIB_FIX_K6 + cv::CALIB_FIX_K5 + cv::CALIB_FIX_K4 + cv::CALIB_FIX_K3;

        cv::Size imgSize(imgWidth, imgHeight);

        result.result = cv::calibrateCamera(obj_pts, im_pts, imgSize, cameraMatrix,
            distortionCoeffs, rotationMatrices, translationVectors, calibFlags);
    } catch (const cv::Exception &e) {
        result.is_cv_exception = 1;
    } catch (...) {
        result.is_other_exception = 1;
    }

    return result;
}

struct cv_return_value_bool find_chessboard_corners_inner(uchar* frameDataRGB, int frameWidth, int frameHeight, int patternWidth, int patternHeight, std::vector<cv::Point2f>* corners ) {
    struct cv_return_value_bool result = { 0, 0, true };

    if (corners==NULL) {
        result.result = false;
        return result;
    }

    try {
        cv::Size patternsize(patternWidth, patternHeight);
        cv::Mat frame(frameHeight, frameWidth, CV_8UC3, frameDataRGB);

        int chessBoardFlags = cv::CALIB_CB_ADAPTIVE_THRESH | cv::CALIB_CB_NORMALIZE_IMAGE | cv::CALIB_CB_FAST_CHECK;
        bool patternfound = cv::findChessboardCorners(frame, patternsize, *corners, chessBoardFlags);

        if (patternfound) {
            // Perform subpixel refinement.
            cv::Mat gray;
            cv::cvtColor(frame, gray, cv::COLOR_BGR2GRAY);

            cv::cornerSubPix(gray, *corners, cv::Size(11, 11), cv::Size(-1, -1),
                cv::TermCriteria( cv::TermCriteria::EPS+cv::TermCriteria::COUNT, 30, 0.1 ));
            result.result = true;
        } else {
            result.result = false;
        }
    } catch (const cv::Exception &e) {
        result.is_cv_exception = 1;
    } catch (...) {
        result.is_other_exception = 1;
    }

    return result;
}

std::vector<cv::Point2f>* vec_point2f_new() {
    return new std::vector<cv::Point2f>;
}

void vec_point2f_delete(std::vector<cv::Point2f>* vec) {
    delete vec;
}

struct cv_return_value_slice vec_point2f_slice(std::vector<cv::Point2f>* vec) {

    struct cv_return_value_slice result = { 0, 0, NULL, 0 };

    if (vec==NULL) {
        result.is_other_exception = 1;
        return result;
    }

    try {
        cv::Point2f* ptr = vec->data();
        result.num_elements = vec->size();
        result.ptr = (void*)ptr;
    } catch (const cv::Exception &e) {
        result.is_cv_exception = 1;
    } catch (...) {
        result.is_other_exception = 1;
    }

    return result;
}

}
