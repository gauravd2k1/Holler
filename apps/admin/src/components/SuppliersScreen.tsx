import { useState } from "react";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { ApiError, createSupplier, listSuppliers, outletId } from "../lib/api";
import { formatMicro, parseToMicro } from "../lib/quantity";

/**
 * Suppliers and pack sizes.
 *
 * This is the screen M6 C5 turns on: a supplier and a pack size created here
 * must make the next goods receipt convert exactly and raise no
 * NO_SUPPLIER_ITEM gap. The falsifier is to receive BEFORE creating them and
 * watch the gap appear, so that its absence afterwards means something.
 *
 * ONE RULE GOVERNS THE PACK-SIZE FORM AND IT IS NOT OBVIOUS:
 * `quantity_dimension` is THE UNIT THE AUTHOR CHOSE, never derived from the
 * item it points at (contracts 0.5.2). If this form auto-filled it from
 * `inventory_item.dimension` the cloud's mismatch check would become `x == x`,
 * could never fire, and would look perfectly correct in review. So the operator
 * picks it, every time, and a wrong pick is caught at write time instead of
 * silently reinterpreting every quantity that follows.
 */
export function SuppliersScreen() {
  const queryClient = useQueryClient();
  const suppliers = useQuery({ queryKey: ["suppliers"], queryFn: listSuppliers });

  if (suppliers.isLoading) return <p>Loading suppliers…</p>;
  if (suppliers.isError) {
    return <p className="error">Could not load suppliers: {String(suppliers.error)}</p>;
  }

  const rows = suppliers.data ?? [];

  return (
    <section>
      <h1>Suppliers and pack sizes</h1>
      <NewSupplierForm
        onCreated={() => void queryClient.invalidateQueries({ queryKey: ["suppliers"] })}
      />
      {rows.length === 0 ? (
        <p>
          No suppliers yet. A goods receipt against a supplier that does not exist is still
          accepted — it records a gap rather than refusing the delivery — so this list being
          empty does not block receiving.
        </p>
      ) : (
        rows.map((supplier) => (
          <article key={supplier.id}>
            <h2>
              {supplier.name} <small>{supplier.code}</small>
            </h2>
            <p>
              {supplier.gstin ?? "no GSTIN"} · {supplier.payment_terms_days} day terms ·{" "}
              {supplier.is_active ? "active" : "inactive"}
            </p>
            <table>
              <thead>
                <tr>
                  <th>Item</th>
                  <th>Purchase unit</th>
                  <th>Pack size</th>
                  <th>Dimension</th>
                  <th>Last price</th>
                </tr>
              </thead>
              <tbody>
                {supplier.items.map((it) => (
                  <tr key={it.id}>
                    <td>{it.inventory_item_id}</td>
                    <td>{it.purchase_unit}</td>
                    <td>{formatMicro(it.pack_size_micro)}</td>
                    {/* Shown, not inferred. See the header. */}
                    <td>{it.quantity_dimension}</td>
                    <td>{it.last_price_paise ?? "—"}</td>
                  </tr>
                ))}
              </tbody>
            </table>
          </article>
        ))
      )}
    </section>
  );
}

const DIMENSIONS = ["MASS", "VOLUME", "COUNT"] as const;

function NewSupplierForm({ onCreated }: { onCreated: () => void }) {
  const [code, setCode] = useState("");
  const [name, setName] = useState("");
  const [itemId, setItemId] = useState("");
  const [purchaseUnit, setPurchaseUnit] = useState("");
  const [packSize, setPackSize] = useState("");
  // NO DEFAULT, DELIBERATELY. An empty selection the operator must resolve is
  // the whole point: a pre-filled dimension is a guess wearing the operator's
  // authority, and it is wrong exactly when it matters.
  const [dimension, setDimension] = useState<string>("");
  const [error, setError] = useState<string | null>(null);

  const create = useMutation({
    mutationFn: async () => {
      if (dimension === "") throw new Error("Choose the dimension the pack size is measured in.");
      const micro = parseToMicro(packSize);
      if (micro === null) throw new Error("Pack size must be a number, e.g. 50 or 1.5");

      return createSupplier({
        supplier: {
          id: crypto.randomUUID(),
          outlet_id: outletId(),
          code,
          name,
          gstin: null,
          phone: null,
          email: null,
          address: null,
          payment_terms_days: 30,
          is_active: true,
          config_version: 0,
          schema_version: 1,
        },
        items: itemId === "" ? [] : [
          {
            id: crypto.randomUUID(),
            supplier_id: undefined,
            inventory_item_id: itemId,
            purchase_unit: purchaseUnit,
            pack_size_micro: micro,
            quantity_dimension: dimension,
            last_price_paise: null,
            is_active: true,
            config_version: 0,
            schema_version: 1,
          },
        ],
      });
    },
    onSuccess: () => {
      setError(null);
      setCode("");
      setName("");
      setItemId("");
      setPurchaseUnit("");
      setPackSize("");
      setDimension("");
      onCreated();
    },
    onError: (e: unknown) => {
      setError(e instanceof ApiError ? `${e.code}: ${e.message}` : String(e));
    },
  });

  return (
    <form
      onSubmit={(e) => {
        e.preventDefault();
        create.mutate();
      }}
    >
      <h2>Add a supplier</h2>
      <label>
        Code <input value={code} onChange={(e) => setCode(e.target.value)} required />
      </label>
      <label>
        Name <input value={name} onChange={(e) => setName(e.target.value)} required />
      </label>

      <fieldset>
        <legend>First pack size (optional)</legend>
        <label>
          Inventory item id <input value={itemId} onChange={(e) => setItemId(e.target.value)} />
        </label>
        <label>
          Purchase unit{" "}
          <input
            value={purchaseUnit}
            onChange={(e) => setPurchaseUnit(e.target.value)}
            placeholder="50kg sack"
          />
        </label>
        <label>
          Pack size <input value={packSize} onChange={(e) => setPackSize(e.target.value)} />
        </label>
        <label>
          Dimension{" "}
          <select value={dimension} onChange={(e) => setDimension(e.target.value)}>
            <option value="">choose…</option>
            {DIMENSIONS.map((d) => (
              <option key={d} value={d}>
                {d}
              </option>
            ))}
          </select>
        </label>
        <p className="hint">
          Choose the unit this pack size is measured in. It is not filled in from the
          inventory item on purpose — that is the check that catches a pack size entered
          against the wrong kind of unit.
        </p>
      </fieldset>

      <button type="submit" disabled={create.isPending}>
        {create.isPending ? "Saving…" : "Create supplier"}
      </button>
      {error !== null && <p className="error">{error}</p>}
    </form>
  );
}
