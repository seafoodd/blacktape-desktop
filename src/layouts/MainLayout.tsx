import React from "react";
import styles from "./main-layout.module.css";

interface MainLayoutProps {
  header?: React.ReactNode;
  leftSidebar?: React.ReactNode;
  rightSidebar?: React.ReactNode;
  footer?: React.ReactNode;
  children: React.ReactNode;
}

export const MainLayout = ({
  header,
  leftSidebar,
  rightSidebar,
  footer,
  children,
}: MainLayoutProps) => {
  return (
    <div className={styles.shell}>
      {header}
      <div className={styles.body}>
        {leftSidebar}
        <main className={styles.main}>{children}</main>
        {rightSidebar}
      </div>
      {footer && <footer className={styles.footer}>{footer}</footer>}
    </div>
  );
};
