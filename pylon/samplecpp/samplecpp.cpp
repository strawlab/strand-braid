#include <pylon/PylonIncludes.h>
#include <pylon/gige/BaslerGigECamera.h>
#include <ostream>
using namespace Pylon;
using namespace Basler_GigECameraParams;
using namespace std;
typedef CBaslerGigECamera Camera_t;
void ProcessImage( unsigned char* pImage, int imageSizeX, int imageSizeY )
{
  // Do something with the image data
}
struct MyContext
{
  // Define some application specific context information here
};
int main()
{
  PylonAutoInitTerm autoInitTerm;
  try
  {
    // Enumerate GigE cameras
    CTlFactory& TlFactory = CTlFactory::GetInstance();
    ITransportLayer *pTl = TlFactory.CreateTl( Camera_t::DeviceClass() );
    DeviceInfoList_t devices;
    if ( 0 == pTl->EnumerateDevices( devices ) ) {
      cerr << "No camera present!" << endl;
      return 1;
    }
    // Create a camera object
    Camera_t Camera ( pTl->CreateDevice( devices[ 0 ] ) );
    // Open the camera object
    Camera.Open();
    // Parameterize the camera
    // Mono8 pixel format
    Camera.PixelFormat.SetValue( PixelFormat_Mono8 );
    // Maximized AOI
    Camera.OffsetX.SetValue( 0 );
    Camera.OffsetY.SetValue( 0 );
    Camera.Width.SetValue( Camera.Width.GetMax() );
    Camera.Height.SetValue( Camera.Height.GetMax() );
    // Continuous mode, no external trigger used
    Camera.TriggerSelector.SetValue( TriggerSelector_AcquisitionStart );
    Camera.TriggerMode.SetValue( TriggerMode_Off );
    Camera.AcquisitionMode.SetValue( AcquisitionMode_Continuous );
    // Configure exposure time and mode
    Camera.ExposureMode.SetValue( ExposureMode_Timed );
    Camera.ExposureTimeRaw.SetValue( 100 );
    // check whether stream grabbers are avalaible
    if (Camera.GetNumStreamGrabberChannels() == 0) {
      cerr << "Camera doesn't support stream grabbers." << endl;
    } else {
      // Get and open a stream grabber
      IStreamGrabber* pGrabber = Camera.GetStreamGrabber(0);
      CBaslerGigECamera::StreamGrabber_t StreamGrabber( Camera.GetStreamGrabber(0) );
      StreamGrabber.Open();
      // Parameterize the stream grabber
      const int bufferSize = (int) Camera.PayloadSize();
      const int numBuffers = 10;
      StreamGrabber.MaxBufferSize = bufferSize;
      StreamGrabber.MaxNumBuffer = numBuffers;
      StreamGrabber.PrepareGrab();
      // Allocate and register image buffers, put them into the
      // grabber's input queue
      unsigned char* ppBuffers[numBuffers];
      MyContext context[numBuffers];
      StreamBufferHandle handles[numBuffers];
      for ( int i = 0; i < numBuffers; ++i )
      {
        ppBuffers[i] = new unsigned char[bufferSize];
        handles[i] = StreamGrabber.RegisterBuffer( ppBuffers[i], bufferSize);
        StreamGrabber.QueueBuffer( handles[i], &context[i] );
      }
      // Start image acquisition
      Camera.AcquisitionStart.Execute();
      // Grab and process 100 images
      const int numGrabs = 100;
      GrabResult Result;
      for ( int i = 0; i < numGrabs; ++i ) {
        // Wait for the grabbed image with a timeout of 3 seconds
        if ( StreamGrabber.GetWaitObject().Wait( 3000 )) {
          // Get an item from the grabber's output queue
          if ( ! StreamGrabber.RetrieveResult( Result ) ) {
            cerr << "Failed to retrieve an item from the output queue" << endl;
            break;
          }
          if ( Result.Succeeded() ) {
            // Grabbing was successful. Process the image.
            ProcessImage( (unsigned char*) Result.Buffer(), Result.GetSizeX(), Result.GetSizeY() );
          } else {
            cerr << "Grab failed: " << Result.GetErrorDescription() << endl;
            break;
          }
          // Requeue the buffer
          if ( i + numBuffers < numGrabs )
            StreamGrabber.QueueBuffer( Result.Handle(), Result.Context() );
        } else {
          cerr << "timeout occurred when waiting for a grabbed image" << endl;
          break;
        }
      }
      // Finished. Stop grabbing and do clean-up
      // The camera is in continuous mode, stop image acquisition
      Camera.AcquisitionStop.Execute();
      // Flush the input queue, grabbing may have failed
      StreamGrabber.CancelGrab();
      // Consume all items from the output queue
      while ( StreamGrabber.GetWaitObject().Wait(0) ) {
        StreamGrabber.RetrieveResult( Result );
        if ( Result.Status() == Canceled )
          cout << "Got canceled buffer" << endl;
      }
      // Deregister and free buffers
      for ( int i = 0; i < numBuffers; ++i ) {
        StreamGrabber.DeregisterBuffer(handles[i]);
        delete [] ppBuffers[i];
      }
      // Clean up
      StreamGrabber.FinishGrab();
      StreamGrabber.Close();
    }
    Camera.Close();
    TlFactory.ReleaseTl( pTl );
  }
  catch( Pylon::GenericException &e )
  {
    // Error handling
    cerr << "An exception occurred!" << endl << e.GetDescription() << endl;
    return 1;
  }
  // Quit application
  return 0;
}
