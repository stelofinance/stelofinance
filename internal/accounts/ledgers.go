package accounts

type LedgerCode int32

const (
	// 0-99 Digital or Derivation items
	// these ledger codes are for items that don't exist in the BitCraft world,
	// or at least not directly

	// For purely digital items
	DigitalItem LedgerCode = 0

	// For items that are a derivation of another item (backed)
	DerivationItem LedgerCode = 1

	// 100-199 Items
	// these ledger codes are for items that directly exist in the BitCraft world

	// For regular in-game items
	Item LedgerCode = 100
	// Maybe add stackable vs non-stackable?
	// Also maybe add cargo items? If these even get added
)
