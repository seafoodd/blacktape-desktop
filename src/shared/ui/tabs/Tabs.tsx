import React from "react";
import clsx from "clsx";
import styles from "./tabs.module.css";

export interface TabItem<T extends string = string> {
  id: T;
  label: string;
}

export interface TabsProps<T extends string = string> {
  tabs: readonly TabItem<T>[];
  activeTab: T;
  onChange: (id: T) => void;
  className?: string;
}

export const Tabs = <T extends string>({
  tabs,
  activeTab,
  onChange,
  className,
}: TabsProps<T>) => {
  const activeIndex = tabs.findIndex((tab) => tab.id === activeTab);

  return (
    <div
      role="tablist"
      className={clsx(styles.tabList, className)}
      style={
        {
          "--active-index": activeIndex >= 0 ? activeIndex : 0,
          "--tab-count": tabs.length,
        } as React.CSSProperties
      }
    >
      {tabs.map((tab) => (
        <button
          key={tab.id}
          role="tab"
          type="button"
          aria-selected={activeTab === tab.id}
          onClick={() => onChange(tab.id)}
          className={clsx(styles.tab, {
            [styles.active]: activeTab === tab.id,
          })}
        >
          {tab.label}
        </button>
      ))}
    </div>
  );
};
