enum EWeaponType
{
	WT_Sword,
	WT_Axe,
	WT_Bow
}

class CWeapon
{
	var weaponType : EWeaponType;
	var damage : int;

	function GetDamage() : int
	{
		return damage;
	}
}
