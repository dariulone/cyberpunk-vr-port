#pragma once
#include <cstdint>
namespace xrfg {
enum class BridgeFlightOperation { steamvr_delivery, steamvr_delivery_attach };
void RecordDeliveryEvent(BridgeFlightOperation,int64_t,uint64_t,uint64_t,uint64_t);
struct DeliveryLogger { void event(BridgeFlightOperation op,int64_t result,uint64_t a=0,uint64_t b=0,uint64_t c=0) const {RecordDeliveryEvent(op,result,a,b,c);} };
inline DeliveryLogger bridge_flight_logger(){return {};}
}
