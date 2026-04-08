import { Link, NavLink } from 'react-router-dom'
import styles from './Header.module.css'

export default function Header() {
  return (
    <header className={styles.header}>
      <Link to="/" className={styles.logo}>
        <span className={styles.star}>★</span>
        cute trading
        <span className={styles.star}>★</span>
      </Link>
      <p className={styles.sub}>polymarket predictions</p>
      <nav className={styles.nav}>
        <NavLink
          to="/"
          end
          className={({ isActive }) => `${styles.navLink} ${isActive ? styles.navLinkActive : ''}`}
        >
          markets
        </NavLink>
        <NavLink
          to="/traders"
          className={({ isActive }) => `${styles.navLink} ${isActive ? styles.navLinkActive : ''}`}
        >
          smart money
        </NavLink>
      </nav>
    </header>
  )
}
