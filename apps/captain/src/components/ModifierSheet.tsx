import { useState } from "react";
import type { CaptainMenuItem, CaptainModifier } from "../lib/api";
import { freeModifierGroups, groupRequiresSelection, modifierSelectionSatisfied } from "../lib/cart";

interface Props {
  item: CaptainMenuItem;
  onCancel: () => void;
  onConfirm: (selected: CaptainModifier[]) => void;
}

/**
 * The free-modifier picker. Shown only when an item has at least one group
 * with at least one free option (docs/captain-api.md: "only free modifiers
 * are selectable"; only groups with a free option to offer reach this
 * screen — MenuCartScreen adds a group-less or all-paid item in one tap with
 * no dialog, because a waiter standing at a table is the form factor).
 */
export function ModifierSheet({ item, onCancel, onConfirm }: Props) {
  const groups = freeModifierGroups(item);
  const [selected, setSelected] = useState<Map<string, CaptainModifier[]>>(new Map());

  function toggle(groupName: string, maxSelection: number, option: CaptainModifier) {
    setSelected((prev) => {
      const next = new Map(prev);
      const current = next.get(groupName) ?? [];
      const already = current.some((o) => o.id === option.id);

      if (maxSelection <= 1) {
        // Radio behaviour: tapping the selected option clears it (legal when
        // the group is optional), tapping another replaces it.
        next.set(groupName, already ? [] : [option]);
        return next;
      }

      if (already) {
        next.set(groupName, current.filter((o) => o.id !== option.id));
        return next;
      }
      if (current.length >= maxSelection) return prev; // at cap, ignore the tap
      next.set(groupName, [...current, option]);
      return next;
    });
  }

  const canConfirm = modifierSelectionSatisfied(groups, selected);

  return (
    <div className="modifier-sheet-backdrop">
      <div className="modifier-sheet">
        <h2>{item.name}</h2>
        {groups.map((g) => (
          <div className="modifier-group" key={g.groupName}>
            <div className="modifier-group-name">
              {g.groupName}
              {groupRequiresSelection(g) && <span className="required-tag"> · required</span>}
            </div>
            <div className="modifier-options">
              {g.options.map((o) => {
                const isSelected = (selected.get(g.groupName) ?? []).some((s) => s.id === o.id);
                return (
                  <button
                    key={o.id}
                    type="button"
                    className={`modifier-option ${isSelected ? "selected" : ""}`}
                    onClick={() => toggle(g.groupName, g.maxSelection, o)}
                  >
                    {o.option_name}
                  </button>
                );
              })}
            </div>
          </div>
        ))}
        <div className="modifier-sheet-actions">
          <button type="button" className="btn btn--lg" onClick={onCancel}>
            Cancel
          </button>
          <button
            type="button"
            className="btn btn--primary btn--lg"
            disabled={!canConfirm}
            onClick={() => onConfirm(groups.flatMap((g) => selected.get(g.groupName) ?? []))}
          >
            Add
          </button>
        </div>
      </div>
    </div>
  );
}
