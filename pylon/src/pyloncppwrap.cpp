#include <stdio.h>
#include "pylon/PylonIncludes.h"
#include "gige/GigETransportLayer.h"

struct RefHolder {
    Pylon::CGrabResultImageRef orig_ref;

    RefHolder(Pylon::CGrabResultImageRef _orig_ref) : orig_ref(_orig_ref) {}
};

extern "C" {

enum PylonCppError_t
{
    PYLONCPPWRAP_NO_ERROR=0,
    PYLONCPPWRAP_ERROR_ENUM_NOT_MATCHED,
    PYLONCPPWRAP_ERROR_CALLBACK_FAIL,
    PYLONCPPWRAP_ERROR_NAME_NOT_FOUND,
    PYLONCPPWRAP_ERROR_NULL_POINTER,
    PYLONCPPWRAP_ERROR_PYLON_EXCEPTION,
    PYLONCPPWRAP_ERROR_INVALID_RESULT,
};

#define InterfaceTypeType uint8_t
enum PYLON_CPP_INTERFACE_TYPE
{
    IValue,
    IBase,
    IInteger,
    IBoolean,
    ICommand,
    IFloat,
    IString,
    IRegister,
    ICategory,
    IEnumeration,
    IEnumEntry,
    IPort,
};

#define EGrabStatusType int8_t
enum PYLON_CPP_GRAB_STATUS
{
    _UndefinedGrabStatus = -1,
    Idle,     ///< Currently not used.
    Queued,   ///< Grab request is in the input queue.
    Grabbed,  ///< Grab request terminated successfully. Buffer is filled with data.
    Canceled, ///< Grab request was canceled. Buffer doesn't contain valid data.
    Failed,   ///< Grab request failed. Buffer doesn't contain valid data.
};

#define PixelType int8_t
enum PYLON_CPP_EPIXEL_TYPE
{
    _UndefinedPixelType = -1,
    PixelType_Mono1packed,
    PixelType_Mono2packed,
    PixelType_Mono4packed,

    PixelType_Mono8,
    PixelType_Mono8signed,
    PixelType_Mono10,
    PixelType_Mono10packed,
    PixelType_Mono10p,
    PixelType_Mono12,
    PixelType_Mono12packed,
    PixelType_Mono12p,
    PixelType_Mono16,

    PixelType_BayerGR8,
    PixelType_BayerRG8,
    PixelType_BayerGB8,
    PixelType_BayerBG8,

    PixelType_BayerGR10,
    PixelType_BayerRG10,
    PixelType_BayerGB10,
    PixelType_BayerBG10,

    PixelType_BayerGR12,
    PixelType_BayerRG12,
    PixelType_BayerGB12,
    PixelType_BayerBG12,

    PixelType_RGB8packed,
    PixelType_BGR8packed,

    PixelType_RGBA8packed,
    PixelType_BGRA8packed,

    PixelType_RGB10packed,
    PixelType_BGR10packed,

    PixelType_RGB12packed,
    PixelType_BGR12packed,

    PixelType_RGB16packed,

    PixelType_BGR10V1packed,
    PixelType_BGR10V2packed,

    PixelType_YUV411packed,
    PixelType_YUV422packed,
    PixelType_YUV444packed,

    PixelType_RGB8planar,
    PixelType_RGB10planar,
    PixelType_RGB12planar,
    PixelType_RGB16planar,

    PixelType_YUV422_YUYV_Packed,

    PixelType_BayerGR12Packed,
    PixelType_BayerRG12Packed,
    PixelType_BayerGB12Packed,
    PixelType_BayerBG12Packed,

    PixelType_BayerGR10p,
    PixelType_BayerRG10p,
    PixelType_BayerGB10p,
    PixelType_BayerBG10p,

    PixelType_BayerGR12p,
    PixelType_BayerRG12p,
    PixelType_BayerGB12p,
    PixelType_BayerBG12p,

    PixelType_BayerGR16,
    PixelType_BayerRG16,
    PixelType_BayerGB16,
    PixelType_BayerBG16,

    PixelType_RGB12V1packed,

    PixelType_Double,
};


#define PYLONCALL(code)                                      \
    try                                                      \
    {                                                        \
        code                                                 \
    }                                                        \
    catch (const Pylon::GenericException &e)                 \
    {                                                        \
        std::cerr << "GenericException during call to Pylon: " \
                  << e.GetDescription() << std::endl;        \
        return PYLONCPPWRAP_ERROR_PYLON_EXCEPTION;                        \
    }

#define PYLONCALL_ERR_DESC(code,buf,n)                       \
    try                                                      \
    {                                                        \
        code                                                 \
    }                                                        \
    catch (const Pylon::GenericException &e)                 \
    {                                                        \
        strncpy(buf,e.GetDescription(),n);                   \
        return PYLONCPPWRAP_ERROR_PYLON_EXCEPTION;                        \
    }

#define NULLCHECK(var)             \
    if (var == NULL)               \
    {                              \
        return PYLONCPPWRAP_ERROR_NULL_POINTER; \
    }

// pointer to a std::string with the type erased to use in C API
typedef void* StdStringPtr;

StdStringPtr
CppStdString_new()
{
    return new std::string();
}

void
CppStdString_delete(StdStringPtr me)
{
    delete (std::string*)me;
}

const char*
CppStdString_bytes(StdStringPtr me)
{
    return ((std::string*)me)->c_str();
}

PylonCppError_t
Pylon_initialize()
{
    PYLONCALL(Pylon::PylonInitialize();)
    return PYLONCPPWRAP_NO_ERROR;
}

PylonCppError_t
Pylon_getVersionString(const char ** sptr)
{
    PYLONCALL(*sptr = Pylon::VersionInfo::getVersionString();)
    return PYLONCPPWRAP_NO_ERROR;
}

PylonCppError_t Pylon_terminate() {
    PYLONCALL(Pylon::PylonTerminate();)
    return PYLONCPPWRAP_NO_ERROR;
}

PylonCppError_t CPylon_new_tl_factory(Pylon::CTlFactory** handle) {
    NULLCHECK(handle);
    PYLONCALL(
        Pylon::CTlFactory &tlFactory = Pylon::CTlFactory::GetInstance();
        *handle = &tlFactory;
    )
    return PYLONCPPWRAP_NO_ERROR;
}

typedef void *enumerate_device_arg0_type;
typedef uint8_t (*enumerate_device_func_type)(enumerate_device_arg0_type, Pylon::CDeviceInfo *);

PylonCppError_t CTlFactory_enumerate_devices(Pylon::CTlFactory * tl_factory,
    enumerate_device_func_type enumerate_device_func,
    enumerate_device_arg0_type arg0)
{
    Pylon::CTlFactory &tlFactory = *tl_factory;
    Pylon::DeviceInfoList_t devices;
    PYLONCALL(tlFactory.EnumerateDevices(devices);)

    for (Pylon::DeviceInfoList_t::iterator it = devices.begin(); it != devices.end(); ++it)
    {
        Pylon::CDeviceInfo* info;
        PYLONCALL(info = new Pylon::CDeviceInfo(*it);) // make copy

        if (enumerate_device_func(arg0, info) != 0)
        {
            return PYLONCPPWRAP_ERROR_CALLBACK_FAIL;
        }
    }
    return PYLONCPPWRAP_NO_ERROR;
}

PylonCppError_t CTlFactory_create_gige_transport_layer(Pylon::CTlFactory *tl_factory,
                                                       Pylon::IGigETransportLayer **handle)
{
    NULLCHECK(tl_factory);
    NULLCHECK(handle);
    PYLONCALL(*handle = dynamic_cast<Pylon::IGigETransportLayer*>(tl_factory->CreateTl(Pylon::BaslerGigEDeviceClass));)
    return PYLONCPPWRAP_NO_ERROR;
}

PylonCppError_t IGigETransportLayer_node_map(Pylon::IGigETransportLayer *tl, GenApi::INodeMap **val)
{
    NULLCHECK(tl);
    NULLCHECK(val);
    PYLONCALL(*val = tl->GetNodeMap();)
    return PYLONCPPWRAP_NO_ERROR;
}

PylonCppError_t CTlFactory_create_device(Pylon::CTlFactory *tl_factory,
                                 const Pylon::CDeviceInfo* info,
                                 Pylon::IPylonDevice **handle,
                                 char* err_msg,
                                 int err_msg_max_len
                                 )
{
    NULLCHECK(tl_factory);
    NULLCHECK(handle);
    PYLONCALL_ERR_DESC(*handle = tl_factory->CreateDevice(*info);, err_msg, err_msg_max_len);
    return PYLONCPPWRAP_NO_ERROR;
}

// TODO investigate switch to CInstantCamera::CInstantCamera( IPylonDevice* pDevice, ECleanup cleanupProcedure = Cleanup_Delete);
PylonCppError_t IPylonDevice_open(Pylon::IPylonDevice* device, uint64_t mode_set) {
    NULLCHECK(device);
    PYLONCALL(
        Pylon::AccessModeSet ms = Pylon::AccessModeSet(mode_set);
        device->Open(ms);
    )
    return PYLONCPPWRAP_NO_ERROR;
}

PylonCppError_t IPylonDevice_close(Pylon::IPylonDevice* device) {
    NULLCHECK(device);
    PYLONCALL(device->Close();)
    return PYLONCPPWRAP_NO_ERROR;
}

PylonCppError_t IPylonDevice_num_stream_grabber_channels(Pylon::IPylonDevice *device, uint64_t* val)
{
    NULLCHECK(device);
    PYLONCALL(*val = device->GetNumStreamGrabberChannels();)
    return PYLONCPPWRAP_NO_ERROR;
}

PylonCppError_t IPylonDevice_stream_grabber(Pylon::IPylonDevice *device, uint64_t index, Pylon::IStreamGrabber **val)
{
    NULLCHECK(device);
    PYLONCALL(*val = device->GetStreamGrabber(index);)
    return PYLONCPPWRAP_NO_ERROR;
}

// TODO investigate switch to virtual GENAPI_NAMESPACE::INodeMap& CInstantCamera::GetTLNodeMap();
// or virtual GENAPI_NAMESPACE::INodeMap& CInstantCamera::GetStreamGrabberNodeMap();
// or GetEventGrabberNodeMap
PylonCppError_t IPylonDevice_node_map(Pylon::IPylonDevice *device, GenApi::INodeMap **val)
{
    NULLCHECK(device);
    NULLCHECK(val);
    PYLONCALL(*val = device->GetNodeMap();)
    return PYLONCPPWRAP_NO_ERROR;
}

typedef void *enumerate_node_arg0_type;
typedef uint8_t (*enumerate_node_func_type)(enumerate_node_arg0_type, GenApi::INode *);

PylonCppError_t INodeMap_get_nodes(GenApi::INodeMap *node_map,
                           enumerate_node_func_type enumerate_node_func,
                           enumerate_node_arg0_type arg0)
{
    NULLCHECK(node_map);
    GenApi::INodeMap &nodeMap = *node_map;
    GenApi::NodeList_t nodes;
    PYLONCALL(nodeMap.GetNodes(nodes);)

    for (GenApi::NodeList_t::iterator it = nodes.begin(); it != nodes.end(); ++it)
    {
        // FIXME Here we are referencing memory we do not own...
        GenApi::INode *inode = *it;
        // printf("made raw pointer to GenApi::INode at %p\n", inode);

        if (enumerate_node_func(arg0, inode) != 0)
        {
            return PYLONCPPWRAP_ERROR_CALLBACK_FAIL;
        }
    }
    return PYLONCPPWRAP_NO_ERROR;
}

PylonCppError_t INodeMap_node(GenApi::INodeMap *node_map, char* name, GenApi::INode **val)
{
    NULLCHECK(node_map);
    NULLCHECK(val);
    PYLONCALL(*val = node_map->GetNode(name);)
    // Hmm, pylon does not throw exception here, check for null result.
    // printf("got node at %p\n", *val);
    if (*val == NULL) {
        return PYLONCPPWRAP_ERROR_NAME_NOT_FOUND;
    }
    return PYLONCPPWRAP_NO_ERROR;
}

PylonCppError_t INode_get_name(GenApi::INode *node, bool fully_qualified, char *dest, size_t maxlen)
{
    NULLCHECK(node);
    GENICAM_NAMESPACE::gcstring result;
    PYLONCALL(result = node->GetName(fully_qualified);)
    strncpy(dest, result.c_str(), maxlen);
    return PYLONCPPWRAP_NO_ERROR;
}

PylonCppError_t INode_get_visibility(GenApi::INode *node, int8_t* visibility)
{
    NULLCHECK(node);
    PYLONCALL(*visibility = node->GetVisibility();)
    return PYLONCPPWRAP_NO_ERROR;
}

PylonCppError_t INode_principal_interface_type(GenApi::INode *node, InterfaceTypeType *val)
{
    NULLCHECK(node);
    GenApi::EInterfaceType type;
    PYLONCALL(type = node->GetPrincipalInterfaceType();)
    switch (type) {
    case GenApi::intfIValue:
        *val = IValue;
        break;
    case GenApi::intfIBase:
        *val = IBase;
        break;
    case GenApi::intfIInteger:
        *val = IInteger;
        break;
    case GenApi::intfIBoolean:
        *val = IBoolean;
        break;
    case GenApi::intfICommand:
        *val = ICommand;
        break;
    case GenApi::intfIFloat:
        *val = IFloat;
        break;
    case GenApi::intfIString:
        *val = IString;
        break;
    case GenApi::intfIRegister:
        *val = IRegister;
        break;
    case GenApi::intfICategory:
        *val = ICategory;
        break;
    case GenApi::intfIEnumeration:
        *val = IEnumeration;
        break;
    case GenApi::intfIEnumEntry:
        *val = IEnumEntry;
        break;
    case GenApi::intfIPort:
        *val = IPort;
        break;
    default:
        return PYLONCPPWRAP_ERROR_ENUM_NOT_MATCHED;
    }
    return PYLONCPPWRAP_NO_ERROR;
}

PylonCppError_t INode_to_integer_node(GenApi::INode **node_handle, GenApi::IInteger **val)
{
    NULLCHECK(node_handle);
    NULLCHECK(*node_handle);
    GenApi::INode *node = *node_handle;
    *val = dynamic_cast<GenApi::IInteger *>(node);
    *node_handle = NULL;
    return PYLONCPPWRAP_NO_ERROR;
}

PylonCppError_t INode_to_boolean_node(GenApi::INode **node_handle, GenApi::IBoolean **val)
{
    NULLCHECK(node_handle);
    NULLCHECK(*node_handle);
    GenApi::INode *node = *node_handle;
    *val = dynamic_cast<GenApi::IBoolean *>(node);
    *node_handle = NULL;
    return PYLONCPPWRAP_NO_ERROR;
}

PylonCppError_t INode_to_float_node(GenApi::INode **node_handle, GenApi::IFloat **val)
{
    NULLCHECK(node_handle);
    NULLCHECK(*node_handle);
    GenApi::INode *node = *node_handle;
    *val = dynamic_cast<GenApi::IFloat *>(node);
    *node_handle = NULL;
    return PYLONCPPWRAP_NO_ERROR;
}

PylonCppError_t INode_to_string_node(GenApi::INode **node_handle, GenApi::IString **val)
{
    NULLCHECK(node_handle);
    NULLCHECK(*node_handle);
    GenApi::INode *node = *node_handle;
    *val = dynamic_cast<GenApi::IString *>(node);
    *node_handle = NULL;
    return PYLONCPPWRAP_NO_ERROR;
}

PylonCppError_t INode_to_enumeration_node(GenApi::INode **node_handle, GenApi::IEnumeration **val)
{
    NULLCHECK(node_handle);
    NULLCHECK(*node_handle);
    GenApi::INode *node = *node_handle;
    *val = dynamic_cast<GenApi::IEnumeration *>(node);
    *node_handle = NULL;
    return PYLONCPPWRAP_NO_ERROR;
}

PylonCppError_t INode_to_command_node(GenApi::INode **node_handle, GenApi::ICommand **val)
{
    NULLCHECK(node_handle);
    NULLCHECK(*node_handle);
    GenApi::INode *node = *node_handle;
    *val = dynamic_cast<GenApi::ICommand *>(node);
    *node_handle = NULL;
    return PYLONCPPWRAP_NO_ERROR;
}

PylonCppError_t IInteger_get_value(GenApi::IInteger *inode, int64_t *val)
{
    NULLCHECK(inode);
    PYLONCALL(*val = inode->GetValue();)
    return PYLONCPPWRAP_NO_ERROR;
}

PylonCppError_t IInteger_get_range(GenApi::IInteger *inode, int64_t *minval, int64_t *maxval)
{
    NULLCHECK(inode);
    PYLONCALL(*minval = inode->GetMin();)
    PYLONCALL(*maxval = inode->GetMax();)
    return PYLONCPPWRAP_NO_ERROR;
}

PylonCppError_t IInteger_set_value(GenApi::IInteger *inode, int64_t val)
{
    NULLCHECK(inode);
    PYLONCALL(inode->SetValue(val);)
    return PYLONCPPWRAP_NO_ERROR;
}

PylonCppError_t IBoolean_get_value(GenApi::IBoolean *inode, bool *val)
{
    NULLCHECK(inode);
    PYLONCALL(*val = inode->GetValue();)
    return PYLONCPPWRAP_NO_ERROR;
}

PylonCppError_t IBoolean_set_value(GenApi::IBoolean *inode, bool val)
{
    NULLCHECK(inode);
    PYLONCALL(inode->SetValue(val);)
    return PYLONCPPWRAP_NO_ERROR;
}

PylonCppError_t IFloat_get_value(GenApi::IFloat *inode, double *val)
{
    NULLCHECK(inode);
    PYLONCALL(*val = inode->GetValue();)
    return PYLONCPPWRAP_NO_ERROR;
}
PylonCppError_t IFloat_get_range(GenApi::IFloat *inode, double *minval, double *maxval)
{
    NULLCHECK(inode);
    PYLONCALL(*minval = inode->GetMin();)
    PYLONCALL(*maxval = inode->GetMax();)
    return PYLONCPPWRAP_NO_ERROR;
}
PylonCppError_t IFloat_set_value(GenApi::IFloat *inode, double val)
{
    NULLCHECK(inode);
    PYLONCALL(inode->SetValue(val);)
    return PYLONCPPWRAP_NO_ERROR;
}

PylonCppError_t IString_get_value(GenApi::IString *inode, char *dest, size_t maxlen)
{
    NULLCHECK(inode);
    GENICAM_NAMESPACE::gcstring result;
    PYLONCALL(result = inode->GetValue();)
    strncpy(dest, result.c_str(), maxlen);
    return PYLONCPPWRAP_NO_ERROR;
}

PylonCppError_t IString_set_value(GenApi::IString *inode, const char *value)
{
    NULLCHECK(inode);
    PYLONCALL(inode->SetValue(value);)
    return PYLONCPPWRAP_NO_ERROR;
}

PylonCppError_t IEnumeration_get_value(GenApi::IEnumeration *enum_node, char *dest, size_t maxlen)
{
    NULLCHECK(enum_node);
    GENICAM_NAMESPACE::gcstring result;
    PYLONCALL(result = **enum_node;)
    strncpy(dest, result.c_str(), maxlen);
    return PYLONCPPWRAP_NO_ERROR;
}

PylonCppError_t IEnumeration_set_value(GenApi::IEnumeration *inode, const char *value)
{
    NULLCHECK(inode);
    GENICAM_NAMESPACE::gcstring val = value;
    PYLONCALL(*inode = val;)
    return PYLONCPPWRAP_NO_ERROR;
}

// See INodeMap_get_nodes
PylonCppError_t IEnumeration_get_entries(GenApi::IEnumeration *node_map,
                           enumerate_node_func_type enumerate_node_func,
                           enumerate_node_arg0_type arg0)
{
    NULLCHECK(node_map);
    GenApi::IEnumeration &nodeMap = *node_map;
    GenApi::NodeList_t nodes;
    PYLONCALL(nodeMap.GetEntries(nodes);)

    for (GenApi::NodeList_t::iterator it = nodes.begin(); it != nodes.end(); ++it)
    {
        // FIXME Here we are referencing memory we do not own...
        GenApi::INode *inode = *it;
        // printf("made raw pointer to GenApi::INode at %p\n", inode);

        if (enumerate_node_func(arg0, inode) != 0)
        {
            return PYLONCPPWRAP_ERROR_CALLBACK_FAIL;
        }
    }
    return PYLONCPPWRAP_NO_ERROR;
}

PylonCppError_t ICommand_execute(GenApi::ICommand *cnode)
{
    NULLCHECK(cnode);
    PYLONCALL(cnode->Execute();)
    return PYLONCPPWRAP_NO_ERROR;
}

PylonCppError_t CDeviceInfo_delete(Pylon::CDeviceInfo *info)
{
    NULLCHECK(info);
    PYLONCALL(delete info;)
    return PYLONCPPWRAP_NO_ERROR;
}

/*
    pub fn IProperties_get_property_names(prop: *const CDeviceInfo,
                                          cb: extern "C" fn(*mut c_void, *mut c_char) -> u8,
                                          target: *mut c_void)
                                          -> PylonCppError_t;

typedef void *enumerate_device_arg0_type;
typedef uint8_t (*enumerate_device_func_type)(enumerate_device_arg0_type, Pylon::CDeviceInfo *);

PylonCppError_t CTlFactory_enumerate_devices(Pylon::CTlFactory * tl_factory,
    enumerate_device_func_type enumerate_device_func,
    enumerate_device_arg0_type arg0)
{

    Pylon::CTlFactory &tlFactory = *tl_factory;
    Pylon::DeviceInfoList_t devices;
    PYLONCALL(tlFactory.EnumerateDevices(devices);)

    for (Pylon::DeviceInfoList_t::iterator it = devices.begin(); it != devices.end(); ++it)
    {
        Pylon::CDeviceInfo* info;
        PYLONCALL(info = new Pylon::CDeviceInfo(*it);) // make copy

        if (enumerate_device_func(arg0, info) != 0)
        {
            return PYLONCPPWRAP_ERROR_CALLBACK_FAIL;
        }
    }
    return PYLONCPPWRAP_NO_ERROR;


}


*/

typedef void *get_property_name_arg0_type;
typedef uint8_t (*get_property_name_func_type)(get_property_name_arg0_type, const char*);

PylonCppError_t IProperties_get_property_names(Pylon::IProperties *prop,
                                               get_property_name_func_type get_property_name_func,
                                               get_property_name_arg0_type arg0)
{
    NULLCHECK(prop);
    Pylon::StringList_t names;
    PYLONCALL(prop->GetPropertyNames(names);) // Return value is not documented. Seems to be num names.

    for (Pylon::StringList_t::iterator it = names.begin(); it != names.end(); ++it)
    {
        const char* cstr = it->c_str();
        if (get_property_name_func(arg0, cstr) != 0)
        {
            return PYLONCPPWRAP_ERROR_CALLBACK_FAIL;
        }
    }
    return PYLONCPPWRAP_NO_ERROR;
}


PylonCppError_t
IProperties_get_property_value(Pylon::IProperties *prop, const char *c_name, char *value, size_t maxlen)
{
    NULLCHECK(prop);
    NULLCHECK(value);
    Pylon::String_t result;
    Pylon::String_t name;
    PYLONCALL(name = Pylon::String_t(c_name);)
    bool ok;
    PYLONCALL(ok = prop->GetPropertyValue(name, result);)
    if (!ok)
    {
        return PYLONCPPWRAP_ERROR_NAME_NOT_FOUND;
    }
    strncpy(value, result.c_str(), maxlen);
    return PYLONCPPWRAP_NO_ERROR;
    }

    PylonCppError_t IStreamGrabber_open(Pylon::IStreamGrabber *grabber)
    {
        NULLCHECK(grabber);
        PYLONCALL(grabber->Open();)
        return PYLONCPPWRAP_NO_ERROR;
    }

    PylonCppError_t IStreamGrabber_close(Pylon::IStreamGrabber *grabber)
    {
        NULLCHECK(grabber);
        PYLONCALL(grabber->Close();)
        return PYLONCPPWRAP_NO_ERROR;
    }

    PylonCppError_t IStreamGrabber_node_map(Pylon::IStreamGrabber *grabber, GenApi::INodeMap **val)
    {
        NULLCHECK(grabber);
        NULLCHECK(val);
        PYLONCALL(*val = grabber->GetNodeMap();)
        return PYLONCPPWRAP_NO_ERROR;
    }

    PylonCppError_t IStreamGrabber_prepare_grab(Pylon::IStreamGrabber *grabber)
    {
        NULLCHECK(grabber);
        PYLONCALL(grabber->PrepareGrab();)
        return PYLONCPPWRAP_NO_ERROR;
    }

    PylonCppError_t IStreamGrabber_cancel_grab(Pylon::IStreamGrabber *grabber)
    {
        NULLCHECK(grabber);
        PYLONCALL(grabber->CancelGrab();)
        return PYLONCPPWRAP_NO_ERROR;
    }

    PylonCppError_t IStreamGrabber_finish_grab(Pylon::IStreamGrabber *grabber)
    {
        NULLCHECK(grabber);
        PYLONCALL(grabber->FinishGrab();)
        return PYLONCPPWRAP_NO_ERROR;
    }

    PylonCppError_t IStreamGrabber_register_buffer(Pylon::IStreamGrabber *grabber, char *buffer, size_t buffer_size, Pylon::StreamBufferHandle *result)
    {
        NULLCHECK(grabber);
        PYLONCALL(*result = grabber->RegisterBuffer(buffer, buffer_size);)
        return PYLONCPPWRAP_NO_ERROR;
    }

    PylonCppError_t IStreamGrabber_queue_buffer(Pylon::IStreamGrabber *grabber,
        Pylon::StreamBufferHandle handle,
        char* err_msg,
        int err_msg_max_len
        )
    {
        NULLCHECK(grabber);
        PYLONCALL_ERR_DESC(grabber->QueueBuffer(handle);, err_msg, err_msg_max_len);
        return PYLONCPPWRAP_NO_ERROR;
    }

    PylonCppError_t IStreamGrabber_get_wait_object(Pylon::IStreamGrabber *grabber, Pylon::WaitObject** handle)
    {
        NULLCHECK(grabber);
        NULLCHECK(handle);
        PYLONCALL(
            Pylon::WaitObject &ref = grabber->GetWaitObject();
            *handle = &ref;)
        return PYLONCPPWRAP_NO_ERROR;
    }

    PylonCppError_t IStreamGrabber_retrieve_result(Pylon::IStreamGrabber *grabber, Pylon::GrabResult **result, bool* is_ready)
    {
        NULLCHECK(grabber);
        NULLCHECK(result);
        Pylon::GrabResult *gr;
        PYLONCALL(gr = new Pylon::GrabResult();)
        PYLONCALL(*is_ready = grabber->RetrieveResult(*gr);)
        if (*is_ready) {
            *result = gr;
        } else {
            delete gr;
        }
        return PYLONCPPWRAP_NO_ERROR;
    }
    PylonCppError_t GrabResult_get_buffer(Pylon::GrabResult* result, char** handle, int64_t* size) {
        NULLCHECK(result);
        NULLCHECK(handle);
        PYLONCALL(*handle = (char *)result->Buffer();)
        PYLONCALL(*size = result->GetPayloadSize();)
        return PYLONCPPWRAP_NO_ERROR;
    }
    PylonCppError_t GrabResult_get_payload_type(Pylon::GrabResult* result, Pylon::EPayloadType* payload_type) {
        NULLCHECK(result);
        PYLONCALL(*payload_type = result->GetPayloadType();)
        return PYLONCPPWRAP_NO_ERROR;
    }

    PylonCppError_t GrabResult_delete(Pylon::GrabResult* result) {
        NULLCHECK(result);
        PYLONCALL(delete result;)
        return PYLONCPPWRAP_NO_ERROR;
    }

    PylonCppError_t GrabResult_status(Pylon::GrabResult *gr, EGrabStatusType* result)
    {
        NULLCHECK(gr);
        Pylon::EGrabStatus st;
        PYLONCALL(st = gr->Status();)
        switch (st)
        {
        case Pylon::_UndefinedGrabStatus:
            *result = _UndefinedGrabStatus;
            break;
        case Pylon::Idle:
            *result = Idle;
            break;
        case Pylon::Queued:
            *result = Queued;
            break;
        case Pylon::Grabbed:
            *result = Grabbed;
            break;
        case Pylon::Canceled:
            *result = Canceled;
            break;
        case Pylon::Failed:
            *result = Failed;
            break;
        default:
            return PYLONCPPWRAP_ERROR_ENUM_NOT_MATCHED;
        }
        return PYLONCPPWRAP_NO_ERROR;
    }

    PylonCppError_t GrabResult_error_code(Pylon::GrabResult *gr, uint32_t* result) {
        NULLCHECK(gr);
        PYLONCALL(*result = gr->GetErrorCode();)
        return PYLONCPPWRAP_NO_ERROR;
    }

    PylonCppError_t GrabResult_error_description(Pylon::GrabResult *gr, StdStringPtr result) {
        NULLCHECK(gr);
        PYLONCALL( *(std::string*)result = gr->GetErrorDescription();)
        return PYLONCPPWRAP_NO_ERROR;
    }

    PylonCppError_t GrabResult_payload_size(Pylon::GrabResult *gr, size_t *result)
    {
        NULLCHECK(gr);
        PYLONCALL(*result = gr->GetPayloadSize_t();)
        return PYLONCPPWRAP_NO_ERROR;
    }

    PylonCppError_t GrabResult_size_x(Pylon::GrabResult *gr, int32_t *result)
    {
        NULLCHECK(gr);
        PYLONCALL(*result = gr->GetSizeX();)
        return PYLONCPPWRAP_NO_ERROR;
    }

    PylonCppError_t GrabResult_size_y(Pylon::GrabResult *gr, int32_t *result)
    {
        NULLCHECK(gr);
        PYLONCALL(*result = gr->GetSizeY();)
        return PYLONCPPWRAP_NO_ERROR;
    }
    // TODO how to get chunk data?
    PylonCppError_t GrabResult_time_stamp(Pylon::GrabResult *gr, uint64_t *result)
    {
        NULLCHECK(gr);
        PYLONCALL(*result = gr->GetTimeStamp();)
        return PYLONCPPWRAP_NO_ERROR;
    }
    PylonCppError_t GrabResult_block_id(Pylon::GrabResult *gr, uint64_t *result)
    {
        NULLCHECK(gr);
        PYLONCALL(*result = gr->GetBlockID();)
        if (UINT64_MAX==*result) {
            return PYLONCPPWRAP_ERROR_INVALID_RESULT;
        }
        return PYLONCPPWRAP_NO_ERROR;
    }
    PylonCppError_t GrabResult_image(Pylon::GrabResult *gr, RefHolder **handle)
    {
        uint32_t width1, width2;
        NULLCHECK(gr);
        NULLCHECK(handle);

        PYLONCALL(
            *handle = new RefHolder(gr->GetImage());
        )
        return PYLONCPPWRAP_NO_ERROR;
    }
    PylonCppError_t GrabResult_handle(Pylon::GrabResult *gr, Pylon::StreamBufferHandle *result)
    {
        NULLCHECK(gr);
        PYLONCALL(*result = gr->Handle();)
        return PYLONCPPWRAP_NO_ERROR;
    }

    void
    RefHolder_delete(RefHolder* me)
    {
        delete me;
    }

    PylonCppError_t
    CGrabResultImageRef_is_valid(RefHolder *handle, bool *result)
    {
        NULLCHECK(handle);
        // IsValid does not throw exception according to docs
        *result = handle->orig_ref.IsValid();
        return PYLONCPPWRAP_NO_ERROR;
    }

    PylonCppError_t
    CGrabResultImageRef_get_pixel_type(RefHolder *handle, PixelType *result)
    {
        NULLCHECK(handle);
        // GetPixelType does not throw exception according to docs
        Pylon::EPixelType type = handle->orig_ref.GetPixelType();
        switch (type) {
            case Pylon::PixelType_Mono1packed: *result = PixelType_Mono1packed; break;
            case Pylon::PixelType_Mono2packed: *result = PixelType_Mono2packed; break;
            case Pylon::PixelType_Mono4packed: *result = PixelType_Mono4packed; break;
            case Pylon::PixelType_Mono8: *result = PixelType_Mono8; break;
            case Pylon::PixelType_Mono8signed: *result = PixelType_Mono8signed; break;
            case Pylon::PixelType_Mono10: *result = PixelType_Mono10; break;
            case Pylon::PixelType_Mono10packed: *result = PixelType_Mono10packed; break;
            case Pylon::PixelType_Mono10p: *result = PixelType_Mono10p; break;
            case Pylon::PixelType_Mono12: *result = PixelType_Mono12; break;
            case Pylon::PixelType_Mono12packed: *result = PixelType_Mono12packed; break;
            case Pylon::PixelType_Mono12p: *result = PixelType_Mono12p; break;
            case Pylon::PixelType_Mono16: *result = PixelType_Mono16; break;
            case Pylon::PixelType_BayerGR8: *result = PixelType_BayerGR8; break;
            case Pylon::PixelType_BayerRG8: *result = PixelType_BayerRG8; break;
            case Pylon::PixelType_BayerGB8: *result = PixelType_BayerGB8; break;
            case Pylon::PixelType_BayerBG8: *result = PixelType_BayerBG8; break;
            case Pylon::PixelType_BayerGR10: *result = PixelType_BayerGR10; break;
            case Pylon::PixelType_BayerRG10: *result = PixelType_BayerRG10; break;
            case Pylon::PixelType_BayerGB10: *result = PixelType_BayerGB10; break;
            case Pylon::PixelType_BayerBG10: *result = PixelType_BayerBG10; break;
            case Pylon::PixelType_BayerGR12: *result = PixelType_BayerGR12; break;
            case Pylon::PixelType_BayerRG12: *result = PixelType_BayerRG12; break;
            case Pylon::PixelType_BayerGB12: *result = PixelType_BayerGB12; break;
            case Pylon::PixelType_BayerBG12: *result = PixelType_BayerBG12; break;
            case Pylon::PixelType_RGB8packed: *result = PixelType_RGB8packed; break;
            case Pylon::PixelType_BGR8packed: *result = PixelType_BGR8packed; break;
            case Pylon::PixelType_RGBA8packed: *result = PixelType_RGBA8packed; break;
            case Pylon::PixelType_BGRA8packed: *result = PixelType_BGRA8packed; break;
            case Pylon::PixelType_RGB10packed: *result = PixelType_RGB10packed; break;
            case Pylon::PixelType_BGR10packed: *result = PixelType_BGR10packed; break;
            case Pylon::PixelType_RGB12packed: *result = PixelType_RGB12packed; break;
            case Pylon::PixelType_BGR12packed: *result = PixelType_BGR12packed; break;
            case Pylon::PixelType_RGB16packed: *result = PixelType_RGB16packed; break;
            case Pylon::PixelType_BGR10V1packed: *result = PixelType_BGR10V1packed; break;
            case Pylon::PixelType_BGR10V2packed: *result = PixelType_BGR10V2packed; break;
            case Pylon::PixelType_YUV411packed: *result = PixelType_YUV411packed; break;
            case Pylon::PixelType_YUV422packed: *result = PixelType_YUV422packed; break;
            case Pylon::PixelType_YUV444packed: *result = PixelType_YUV444packed; break;
            case Pylon::PixelType_RGB8planar: *result = PixelType_RGB8planar; break;
            case Pylon::PixelType_RGB10planar: *result = PixelType_RGB10planar; break;
            case Pylon::PixelType_RGB12planar: *result = PixelType_RGB12planar; break;
            case Pylon::PixelType_RGB16planar: *result = PixelType_RGB16planar; break;
            case Pylon::PixelType_YUV422_YUYV_Packed: *result = PixelType_YUV422_YUYV_Packed; break;
            case Pylon::PixelType_BayerGR12Packed: *result = PixelType_BayerGR12Packed; break;
            case Pylon::PixelType_BayerRG12Packed: *result = PixelType_BayerRG12Packed; break;
            case Pylon::PixelType_BayerGB12Packed: *result = PixelType_BayerGB12Packed; break;
            case Pylon::PixelType_BayerBG12Packed: *result = PixelType_BayerBG12Packed; break;
            case Pylon::PixelType_BayerGR10p: *result = PixelType_BayerGR10p; break;
            case Pylon::PixelType_BayerRG10p: *result = PixelType_BayerRG10p; break;
            case Pylon::PixelType_BayerGB10p: *result = PixelType_BayerGB10p; break;
            case Pylon::PixelType_BayerBG10p: *result = PixelType_BayerBG10p; break;
            case Pylon::PixelType_BayerGR12p: *result = PixelType_BayerGR12p; break;
            case Pylon::PixelType_BayerRG12p: *result = PixelType_BayerRG12p; break;
            case Pylon::PixelType_BayerGB12p: *result = PixelType_BayerGB12p; break;
            case Pylon::PixelType_BayerBG12p: *result = PixelType_BayerBG12p; break;
            case Pylon::PixelType_BayerGR16: *result = PixelType_BayerGR16; break;
            case Pylon::PixelType_BayerRG16: *result = PixelType_BayerRG16; break;
            case Pylon::PixelType_BayerGB16: *result = PixelType_BayerGB16; break;
            case Pylon::PixelType_BayerBG16: *result = PixelType_BayerBG16; break;
            case Pylon::PixelType_RGB12V1packed: *result = PixelType_RGB12V1packed; break;
            case Pylon::PixelType_Double: *result = PixelType_Double; break;
            default: return PYLONCPPWRAP_ERROR_ENUM_NOT_MATCHED;
        }
        return PYLONCPPWRAP_NO_ERROR;
    }

    PylonCppError_t
    CGrabResultImageRef_get_width(RefHolder *handle, uint32_t *result)
    {
        NULLCHECK(handle);
        // GetWidth does not throw exception according to docs
        *result = handle->orig_ref.GetWidth();
        if (0==*result) {
            return PYLONCPPWRAP_ERROR_INVALID_RESULT;
        }
        return PYLONCPPWRAP_NO_ERROR;
    }

    PylonCppError_t
    CGrabResultImageRef_get_height(RefHolder *handle, uint32_t *result)
    {
        NULLCHECK(handle);
        // GetHeight does not throw exception according to docs
        *result = handle->orig_ref.GetHeight();
        if (0==*result) {
            return PYLONCPPWRAP_ERROR_INVALID_RESULT;
        }
        return PYLONCPPWRAP_NO_ERROR;
    }

    PylonCppError_t
    CGrabResultImageRef_get_buffer(RefHolder *handle, const void **buffer)
    {
        NULLCHECK(handle);
        // GetBuffer does not throw exception according to docs
        *buffer = handle->orig_ref.GetBuffer();
        NULLCHECK(*buffer);
        return PYLONCPPWRAP_NO_ERROR;
    }

    PylonCppError_t
    CGrabResultImageRef_get_image_size(RefHolder *handle, size_t *result)
    {
        NULLCHECK(handle);
        // GetImageSize does not throw exception according to docs
        *result = handle->orig_ref.GetImageSize();
        if (0==*result) {
            return PYLONCPPWRAP_ERROR_INVALID_RESULT;
        }
        return PYLONCPPWRAP_NO_ERROR;
    }

    PylonCppError_t
    CGrabResultImageRef_get_stride(RefHolder *handle, size_t *result)
    {
        NULLCHECK(handle);
        bool success;
        // GetStride does not throw exception according to docs
        success = handle->orig_ref.GetStride(*result);
        if (!success) {
            return PYLONCPPWRAP_ERROR_INVALID_RESULT;
        }
        return PYLONCPPWRAP_NO_ERROR;
    }

    PylonCppError_t
    WaitObject_wait(Pylon::WaitObject *wait_object, unsigned int timeout_msec, bool *result)
    {
        NULLCHECK(wait_object);
        NULLCHECK(result);
        PYLONCALL(*result = wait_object->Wait(timeout_msec);)
        return PYLONCPPWRAP_NO_ERROR;
    }

}
