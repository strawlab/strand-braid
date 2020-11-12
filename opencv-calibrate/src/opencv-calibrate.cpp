#include "opencv2/calib3d/calib3d.hpp"

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
        // Here we create the data as OpenCV 2 `Mat` data structures
        // (instead of OpenCV 1 `CvMat` structures).

        int i, total = 0;
        for( i = 0; i < image_count; i++ ) {
            total += point_counts[i];
        }

        // Create C++ wrapper/view around externally allocated data.
        cv::Mat objectPoints(1, total, CV_64FC3, (void*)object_points);
        cv::Mat imagePoints(1, total, CV_64FC2, (void*)image_points);
        cv::Mat pointCounts(1, image_count, CV_32S, (void*)point_counts);
        cv::Mat cameraMatrix(3, 3, CV_64F, (void*)camera_matrix);
        cv::Mat distortionCoeffs(5, 1, CV_64F, (void*)distortion_coeffs);
        cv::Mat rotationMatrices(image_count, 9, CV_64F, (void*)rotation_matrices);
        cv::Mat translationVectors(image_count, 3, CV_64F, (void*)translation_vectors);

        // cvCalibrateCamera2 detects size of distortionCoeffs matrix and sets
        // flags appropriately. Furthermore, we are trying to copy the behavior
        // of the ROS `camera_calibration` package `cameracalibrator.py` node (which uses
        // camera_calibration.calibrator.MonoCalibrator`) which sets flags
        // cv2.CALIB_FIX_K6 | cv2.CALIB_FIX_K5 | cv2.CALIB_FIX_K4 | cv2.CALIB_FIX_K3

        int calibFlags = CV_CALIB_FIX_K6 + CV_CALIB_FIX_K5 + CV_CALIB_FIX_K4 + CV_CALIB_FIX_K3;

        cv::Size imgSize(imgWidth, imgHeight);

        // Here we convert `Mat` -> `CvMat` for use with cvCalibrateCamera2 function.
        // According to the docs, this does not copy the data, but just creates
        // a new view of existing data.
        CvMat objectPointsC = objectPoints;
        CvMat imagePointsC = imagePoints;
        CvMat pointCountsC = pointCounts;
        CvSize imgSizeC = imgSize;
        CvMat cameraMatrixC = cameraMatrix;
        CvMat distortionCoeffsC = distortionCoeffs;
        CvMat rotationMatricesC = rotationMatrices;
        CvMat translationVectorsC = translationVectors;

        result.result = cvCalibrateCamera2(&objectPointsC, &imagePointsC, &pointCountsC, imgSizeC,
            &cameraMatrixC, &distortionCoeffsC, &rotationMatricesC, &translationVectorsC,
            calibFlags);
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
        // Default flags `CALIB_CB_ADAPTIVE_THRESH+CALIB_CB_NORMALIZE_IMAGE`.
        result.result = findChessboardCorners(frame, patternsize, *corners);
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
