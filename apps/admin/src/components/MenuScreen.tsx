import { useMemo, useState } from "react";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import type { MenuItem } from "@holler/contracts";
import { listMenuCategories, listMenuItems, patchMenuItem, ApiError } from "../lib/api";
import { formatPaise, parseRupeesToPaise } from "../lib/money";

/**
 * Menu and pricing.
 *
 * THE BANNER AT THE TOP IS NOT DECORATION AND MUST NOT BE REMOVED. Nothing in
 * the product carries a cloud menu edit to a till until that till's next config
 * pull, and before contracts 0.7.0 nothing pulled at all — the function existed
 * with a single caller, a test. The pull is hosted now, on the same periodic
 * loop as the outbox drain, but it is still a pull with an interval and an
 * uplink that is routinely down (ADR-013). So a saved price means "the cloud
 * row changed", never "the shop floor changed", and this screen says which.
 *
 * Same rule ADR-019 applies to purchase-order receipt progress: show both
 * sides, label them, never reconcile them into one number that hides which
 * truth was chosen.
 */
export function MenuScreen() {
  const queryClient = useQueryClient();
  const itemsQuery = useQuery({ queryKey: ["menu", "items"], queryFn: listMenuItems });
  const categoriesQuery = useQuery({
    queryKey: ["menu", "categories"],
    queryFn: listMenuCategories,
  });

  const categoryName = useMemo(() => {
    const byId = new Map((categoriesQuery.data ?? []).map((c) => [c.id, c.name] as const));
    // Falls back to the raw id rather than hiding the item: a category the
    // sync has not delivered yet must not make its items disappear from the
    // one screen that manages them.
    return (id: string) => byId.get(id) ?? id;
  }, [categoriesQuery.data]);

  if (itemsQuery.isLoading) return <p>Loading menu…</p>;
  if (itemsQuery.isError) return <p className="error">Could not load the menu: {String(itemsQuery.error)}</p>;

  const items = itemsQuery.data ?? [];

  return (
    <section>
      <h1>Menu and pricing</h1>
      <p className="replica-note" role="note">
        <strong>Edits here change the cloud, not the tills.</strong> A till applies a
        change on its next config sync, and a till with no uplink keeps selling at the
        price it last received. This screen cannot tell you what any till is currently
        charging.
      </p>
      <table>
        <thead>
          <tr>
            <th>Item</th>
            <th>Category</th>
            <th>Price</th>
            <th>HSN/SAC</th>
            <th>Available on the till</th>
            <th />
          </tr>
        </thead>
        <tbody>
          {items.map((item) => (
            <MenuItemRow
              key={item.id}
              item={item}
              categoryName={categoryName(item.category_id)}
              onSaved={() => void queryClient.invalidateQueries({ queryKey: ["menu", "items"] })}
            />
          ))}
        </tbody>
      </table>
    </section>
  );
}

function MenuItemRow({
  item,
  categoryName,
  onSaved,
}: {
  item: MenuItem;
  categoryName: string;
  onSaved: () => void;
}) {
  const [editing, setEditing] = useState(false);
  const [price, setPrice] = useState(formatPaise(item.base_price_paise));
  const [name, setName] = useState(item.name);
  const [error, setError] = useState<string | null>(null);

  const save = useMutation({
    mutationFn: async () => {
      const paise = parseRupeesToPaise(price);
      if (paise === null) throw new Error("Price must be a number, e.g. 125.50");
      // Only changed fields are sent. A PATCH that restated every field would
      // overwrite a concurrent edit to a field this operator never touched.
      const patch: Record<string, unknown> = {};
      if (name !== item.name) patch.name = name;
      if (paise !== item.base_price_paise) patch.base_price_paise = paise;
      if (Object.keys(patch).length === 0) return item;
      return patchMenuItem(item.id, patch);
    },
    onSuccess: () => {
      setError(null);
      setEditing(false);
      onSaved();
    },
    onError: (e: unknown) => {
      // The machine-readable code first: it is stable and it is what the cloud
      // actually said. The prose is the fallback.
      setError(e instanceof ApiError ? `${e.code}: ${e.message}` : String(e));
    },
  });

  if (!editing) {
    return (
      <tr>
        <td>{item.name}</td>
        <td>{categoryName}</td>
        <td>{formatPaise(item.base_price_paise)}</td>
        <td>{item.hsn_sac ?? <span className="warn">not set — cannot be billed</span>}</td>
        <td>{item.is_available ? "yes" : "no"}</td>
        <td>
          <button type="button" className="btn" onClick={() => setEditing(true)}>
            Edit
          </button>
        </td>
      </tr>
    );
  }

  return (
    <tr>
      <td>
        <input value={name} onChange={(e) => setName(e.target.value)} aria-label="Item name" />
      </td>
      <td>{categoryName}</td>
      <td>
        <input value={price} onChange={(e) => setPrice(e.target.value)} aria-label="Price" />
      </td>
      <td>{item.hsn_sac ?? "—"}</td>
      {/*
        Availability is READ-ONLY here and that is a §50.1 boundary, not an
        omission. `is_available` is authored at the outlet and replayed up; the
        cloud editing it would make two writers of one field.
      */}
      <td>{item.is_available ? "yes" : "no"}</td>
      <td>
        <button type="button" className="btn btn--primary" onClick={() => save.mutate()} disabled={save.isPending}>
          {save.isPending ? "Saving…" : "Save"}
        </button>
        <button type="button" className="btn" onClick={() => setEditing(false)}>
          Cancel
        </button>
        {error !== null && <span className="error">{error}</span>}
      </td>
    </tr>
  );
}
