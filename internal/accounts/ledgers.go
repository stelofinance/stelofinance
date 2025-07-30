package accounts

type LedgerCode int32

const (
	// 0-99 Ditial items
	// these ledger codes are for items that don't exist in the BitCraft world

	// For digital items, like stelo itself
	DigitalItem LedgerCode = 0

	// 100-199 Items
	// these ledger codes are for items that exist in the BitCraft world

	// For regular in-game items
	Item LedgerCode = 100
	// Maybe add stackable vs non-stackable?

	// 200-299 Cargo items
	// These ledger codes are for items that exist in the BitCraft world and are Cargo
	// Still not sure if I want/should add these...
)

func (a LedgerCode) isDepositable() bool {
	switch a {
	case Item:
		return true
	default:
		return false
	}
}
