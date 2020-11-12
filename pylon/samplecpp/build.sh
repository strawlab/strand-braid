g++ samplecpp.cpp -I /opt/pylon5/include -L/opt/pylon5/lib64 \
  -lpylonbase -lpylonutility -lgxapi -lGenApi_gcc_v3_0_Basler_pylon_v5_0 \
  -lGCBase_gcc_v3_0_Basler_pylon_v5_0 -lLog_gcc_v3_0_Basler_pylon_v5_0 \
  -osamplecpp
