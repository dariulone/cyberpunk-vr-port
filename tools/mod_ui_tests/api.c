#include "Overlay/ModUiApi.h"
_Static_assert(sizeof(CvrModUiInput)==24,"C ABI input layout changed");
const CvrModUiApi* request_api(CvrGetModUiApi get){return get(CVR_MOD_UI_VERSION,sizeof(CvrModUiApi));}
