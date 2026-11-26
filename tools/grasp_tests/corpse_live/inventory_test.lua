local root=... or "."
CName={new=function(x)return x end}
dofile(root.."/tools/grasp_tests/corpse_live/bridge.lua")
local difference=CorpseAudit.inventoryDifference
local before={weapon={quantity=1,record="weapon"},ammo={quantity=200,record="ammo"}}
local current={weapon={quantity=1,record="weapon"},ammo={quantity=200,record="ammo"}}
assert(difference(before,current).equal)
current.sessionProp={quantity=1,record="<TDBID:C9A90E1E:19>"}
local delta=difference(before,current)
assert(not delta.equal and #delta.extra==1 and delta.extra[1].key=="sessionProp" and #delta.missing==0)
current.sessionProp=nil;current.ammo.quantity=199;delta=difference(before,current)
assert(not delta.equal and #delta.quantityChanges==1 and delta.quantityChanges[1].before==200 and delta.quantityChanges[1].now==199)
current.ammo.quantity=200;current.weapon=nil;current.replacementWeapon={quantity=1,record="weapon"};delta=difference(before,current)
assert(not delta.equal and #delta.missing==1 and #delta.extra==1,"same record/count concealed a different unique item ID")
print("PASS exact inventory ID/quantity equality; extra Prop, missing item, replaced unique ID and quantity changes block restore")
