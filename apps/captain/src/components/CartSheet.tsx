import { cartTotalPaise, setLineQuantity, type CartLine } from "../lib/cart";
import { formatPaise } from "../lib/money";
import { Empty } from "./Waiting";

interface Props {
  cart: CartLine[];
  onCartChange: (lines: CartLine[]) => void;
  onClose: () => void;
}

/**
 * The cart, as a list the waiter can read and undo.
 *
 * WHY THIS EXISTS. The cart bar showed a count and a total and nothing else:
 * "1 item, ₹695.00". A waiter who tapped twice by accident could not see what
 * was in it, could not tell which dish was doubled, and had no way back except
 * leaving the table and starting again. That is not polish — it is the
 * difference between a tool and a guess, and it shows on camera the first time
 * a tap lands wrong.
 *
 * TOUCH TARGETS ARE 44px. Smaller controls are hit-or-miss one-handed while
 * holding a tray, and a mis-hit here changes an order.
 *
 * NO IDS ON SCREEN. A line is identified to the waiter by dish, variant and
 * modifiers — the same words the kitchen will read — never by its key.
 */
export function CartSheet({ cart, onCartChange, onClose }: Props) {
  const total = cartTotalPaise(cart);

  return (
    <div className="modifier-sheet-backdrop" role="dialog" aria-modal="true" aria-label="Cart">
      <div className="modifier-sheet cart-sheet">
        <div className="cart-sheet__head">
          <h3>This table's order</h3>
          <button type="button" className="btn btn--ghost" onClick={onClose}>
            Done
          </button>
        </div>

        {cart.length === 0 ? (
          <Empty
            title="Nothing added yet."
            detail="Tap a dish on the menu to start this table's order."
          />
        ) : (
          <ul className="cart-lines">
            {cart.map((line) => (
              <li className="cart-line" key={line.key}>
                <div className="cart-line__text">
                  <span className="cart-line__name">{line.itemName}</span>
                  <span className="cart-line__detail">
                    {line.variantName}
                    {line.modifiers.length > 0 &&
                      ` · ${line.modifiers.map((m) => m.option_name).join(", ")}`}
                  </span>
                </div>

                <div className="cart-line__qty">
                  <button
                    type="button"
                    className="qty-btn"
                    aria-label={`One less ${line.itemName}`}
                    onClick={() => onCartChange(setLineQuantity(cart, line.key, line.quantity - 1))}
                  >
                    −
                  </button>
                  <span className="qty-value" aria-live="polite">
                    {line.quantity}
                  </span>
                  <button
                    type="button"
                    className="qty-btn"
                    aria-label={`One more ${line.itemName}`}
                    onClick={() => onCartChange(setLineQuantity(cart, line.key, line.quantity + 1))}
                  >
                    +
                  </button>
                </div>

                <span className="cart-line__money money">
                  {formatPaise(line.unitPricePaise * line.quantity)}
                </span>

                {/* Remove is SEPARATE from stepping down to zero. Both work,
                    but a waiter clearing a wrong tap should not have to press
                    − three times to undo a quantity of three. */}
                <button
                  type="button"
                  className="cart-line__remove"
                  aria-label={`Remove ${line.itemName}`}
                  onClick={() => onCartChange(setLineQuantity(cart, line.key, 0))}
                >
                  Remove
                </button>
              </li>
            ))}
          </ul>
        )}

        <div className="cart-sheet__total">
          <span>Total</span>
          <span className="money">{formatPaise(total)}</span>
        </div>
      </div>
    </div>
  );
}
