class CPlayer
{
	var currentWeapon : CWeapon;
	var health : int;

	function Attack() : int
	{
		return currentWeapon.$0GetDamage();
	}

	function TakeDamage(amount : int)
	{
		health -= amount;
	}
}
