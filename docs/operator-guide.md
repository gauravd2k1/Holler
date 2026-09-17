# Holler — operator guide

How to run a service on Holler, screen by screen. Every button name below is
the word printed on the button, and every screen name is the word in the
navigation.

**Before you start**, someone should have started the system for you. You should
have: the till open and signed in, the Kitchen Display open on its screen, and a
pairing token for each waiter's phone.

Sign-ins:

| Screen | Email | Password |
|---|---|---|
| The till | `cashier@holler.test` | `holler123` |
| Back office | `owner@holler.test` | `holler123` |

**Do not ring up drinks from the bar.** Alcohol is set up at a zero tax rate, so
a bar line prints a tax of ₹0 on the bill. Food only.

**Two words for one thing.** The Kitchen Display says **Accept** and
**Accepted**. The till's Kitchen panel calls the same step **Acknowledge** and
**Acknowledged**. They mean the same thing.

---

## A. One table, from first item to printed bill

1. On the till, go to the main ordering screen. If you are on another screen,
    press **Back to POS** at the top right.
    *You see the menu, with category buttons along the top.*
2. In the top bar, leave the order type on **Dine In**.
    *The Dine In button appears pressed in.*
3. In the **Table** dropdown, choose **Main / T3**.
    *The dropdown now reads "Main / T3" instead of "Choose table".*
4. Type `Thai` into the **Search menu…** box at the top.
    *The menu grid narrows to matching dishes.*
5. Press **Thai Grilled Chicken Salad**.
    *It is added to the cart on the right, showing ₹475.00. If the dish asks
    "Choose a size", pick one first.*
6. Press the **+** button beside that line once.
    *The line reads 2, and the Subtotal doubles to ₹950.00.*
7. Press **Send**, at the bottom of the cart.
    *The cart empties. The kitchen now has the order.*
8. Press **Orders** in the top bar.
    *The Orders list opens. Your order is the top row, with a number like
    #A1, the type Dine In, and a status.*
9. **Go to the Kitchen Display.**
    *A ticket has appeared, showing the station, the dish and the quantity,
    with a minute counter. Its status reads New.*
10. On that ticket, press **Accept**.
    *The ticket's status changes to Accepted.*
11. **Go back to the till, to the Orders list.** Press **Kitchen** on your
    order's row.
     *A panel opens under the row listing the ticket, and its status reads
     Acknowledged. It updates by itself — you do not need to press anything.*
12. **On the Kitchen Display**, press **Start preparing**, then **Mark ready**.
     *The ticket's status goes to Preparing, then Ready.*
13. **On the till**, look at the Kitchen panel again.
     *The ticket now reads Ready.*
14. Press **Bill** on the order's row.
     *The bill screen opens, listing the items.*
15. Press **Issue Bill**.
     *The bill is created and shows Subtotal, tax, Round off and Grand Total.
     A QR code appears under the heading **Scan to pay via UPI**, with the
     amount and the payee under it.*
16. Under **Take Payment**, choose **CASH** in the method dropdown and type the
     full amount into the **Amount ₹** box.
     *"Entered so far" matches the Grand Total and "Remaining after entered
     tenders" reads ₹0.00.*
17. Press **Record Payment(s)**.
     *The payment is listed under Payments. The screen says the bill is fully
     settled.*
18. Press **Print Bill**.
     *The receipt is produced and opens on screen, showing the restaurant's
     name, address and GST number, the items, the tax and the same UPI QR.*
19. **Open the back office** and sign in. Press the **Orders** tab.
     *Your order is listed, with its number, what it was, its status, the item
     count, and the till that took it named under **Taken**.*

> **Splitting a bill?** Use two rows under Take Payment — add one for **CASH**
> and one for **UPI**, each with part of the amount, then press **Record
> Payment(s)** once.

---

## B. Taking an order on a waiter's phone

1. On the phone, open the ordering page (the address ends in **:9320**).
    *The screen reads **Pair this phone**.*
2. Paste the pairing token into the box and press **Pair**.
    *The phone moves to a screen headed **Tables**. If it says the token was
    rejected, see "If something goes wrong" below.*
3. Press **Main / T3**.
    *The menu opens, with a cart button at the bottom.*
4. Press **Thai Grilled Chicken Salad**.
    *The cart count at the bottom goes up by one.*
5. Press the cart at the bottom.
    *A sheet slides up headed **This table's order**, listing the dish and a
    Total.*
6. Press **Send**.
    *The screen reads **Sent to the kitchen**.*
7. Press **Back to tables**.
8. **Check the Kitchen Display.**
    *A ticket has arrived for that table.*
9. Now add a second round. Press **Main / T3** again.
10. Press a different dish — **Pad Thai**.
     *The cart count goes up by one.*
11. Press the cart, then press **Send**.
     *It says **Sent to the kitchen** again.*
12. **Check the Kitchen Display.**
     *There is now a **second ticket**, carrying only the new dish. The first
     ticket is unchanged.*
13. **On the till**, press **Orders**.
     *There is **one** order for Main / T3, not two, and its item count covers
     both rounds.*

> Every waiter's phone should have its own pairing token, so the back office can
> say who took which order. One token shared between phones works, but then
> every order looks like it came from the same person.

---

## C. When the internet drops

The till keeps working with no internet. Orders, kitchen tickets and bills all
carry on; they are sent to the back office once the connection returns.

1. **Turn the laptop's WiFi off.**
2. On the till, take an order as in section A: choose **Main / T3**, add
    **Thai Grilled Chicken Salad**, press **Send**.
    *Nothing about the till changes. The order is taken normally.*
3. **Look at the Kitchen Display.**
    *The ticket arrives as usual — the kitchen screen talks to the till
    directly, not over the internet.*
4. Look at the top of the till screen.
    *A banner appears saying a number of records **will not reach the cloud**,
    or are **still retrying**. This is the till telling you it is holding
    things until the connection is back.*
5. **Turn the WiFi back on.**
6. Wait up to a minute.
    *The banner empties by itself. You do not need to press anything, and you
    must not re-enter the order.*
7. **Open the back office**, press the **Orders** tab.
    *The order you took while offline is listed.*

> A line reading **"… records kept locally — the cloud has no route for them
> yet. Nothing to do."** is normal and stays there. It is not an error and
> nothing is lost.

---

## D. Back office and stock

### Suppliers

1. **Open the back office** and sign in.
2. Press the **Suppliers** tab.
    *The screen reads **Suppliers and pack sizes**, listing your suppliers.*
3. Press the supplier's name to open it.
    *Its items are listed, with the purchase unit, pack size and last price for
    each.*

### Goods receipts

4. Press the **Goods receipts** tab.
    *The screen reads **Goods receipts**, with a note saying this is the
    cloud's copy of what the tills recorded.*
5. Press the receipt in the list to open it.
    *Its lines appear: the item, what was **Entered**, the **Pack size**, the
    **Base quantity** it worked out, and the **Line total**.*

### Recording wastage on the till

6. On the till, press **Stock** in the top bar.
    *The **Current Stock** screen opens, listing every ingredient with its
    **Current Quantity** and **Reorder Level**.*
7. **Find Limes and write down its Current Quantity.**
    *You will compare against this in a moment.*
8. Press **Record Wastage**.
    *A form opens headed **Record Wastage**.*
9. In the item dropdown, choose **Limes**.
    *The quantity box's label changes to **Quantity (whole pieces)**.*
10. Type `1` into the quantity box.
     *The screen restates what you are about to record.*
11. Leave **Reason** on **SPOILAGE**.
12. Press **Record Wastage**.
     *The form confirms it was recorded.*
13. Press **Back to Stock** and find **Limes** again.
     *Its **Current Quantity** is **one less** than the number you wrote down.*
14. Look at the top of the till screen.
     *No red banner line demanding attention. A muted "kept locally" line is
     fine and expected.*

---

## If something goes wrong

**The phone says the order could not be sent.**
Check the Kitchen Display first — if the ticket is there it worked, so carry on;
if it is not, press **Send** once more. If two tickets appear, cancel one on the
till, not on the phone.

**The phone says the device token was rejected.**
Wait a minute and press **Pair** again — a newly issued token takes up to a
minute to reach the till. If it still fails, the token was mistyped or is from
an earlier session; ask for a fresh one.

**A red line on the till's banner says records will not reach the cloud.**
Do not press anything and do not re-enter the order. Note the order number and
carry on serving; the order, the kitchen ticket and the bill are all safe on the
till. Tell whoever supports the system afterwards.

**The Kitchen Display is not showing a ticket.**
Check the connection indicator at the top of the Kitchen Display: if it does not
say connected, reload the page. If it is connected but empty, check on the till
that the order's status says it went to the kitchen — if it did not, press **Send
to Kitchen** on the order's row in the **Orders** list.

**The Kitchen Display says "Not confirmed by kitchen system — try again".**
Press the button again. The till did not confirm the change; pressing it twice
does no harm.

**A bill will not issue.**
Every discount that needs a reason must have one typed in before the bill can be
issued. Check the Discounts section of the bill screen.
